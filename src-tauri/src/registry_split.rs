//! 实例注册表按壳模式**分文件**的一次性拆分。
//!
//! ## 为什么要拆
//!
//! 两个壳共用一份 `state/instances.json` 有三个问题，按严重程度排：
//!
//! 1. **跨进程竞态**。注册表的读-改-写只有进程内互斥（`instance::lifecycle_mutex`
//!    是进程级 Mutex），release 与 dev 是两个进程、两个锁域。两者同时启动时各自
//!    读-改-写同一个文件，后写的那份会把前一份的记录整条盖掉。
//! 2. **一个壳能改到另一个壳的列表**。dev 壳删一个实例，release 壳的顶部页签
//!    下一��就少一项；dev 壳建实例，release 的「所有实例」页签多一项。与
//!    「两个壳各跑各的」相抵触。
//! 3. **共享可变字段**。`default_instance_id` 这类「本壳该服务谁」的指针写在
//!    共享文件里，等于让一个壳的选择影响另一个壳的解析（`instance::default_family`）。
//!
//! 拆成两份文件之后，1/2 消失，3 变成「谁写都只影响自己」——原先那套「只有 release
//! 能写共享指针」的防御性限制随之没有存在理由。
//!
//! ## 拆分规则（幂等、可重复跑）
//!
//! - **认领**：本模式的文件不存在时，从共享的 `state/instances.json` 拷一份过来。
//!   拷而不是搬——另一个壳还没认领时，它也得能读到同样的记录。
//! - **让位**：自己这份里如果还有**另一个壳的默认实例**记录，而对方已经认领过
//!   （对方的文件存在），就从自己这份里删掉。删之前先确认对方那份确实存在，
//!   否则把记录删了对方就再也读不到了（`~/.dsh-xlink/kernels/<family>/instances/<id>/`
//!   里的数据不受影响，下次启动会重建记录，只是 `created_at_ms` 刷新）。
//! - 顺序无关：dev 先跑、release 先跑都能收敛到「各持一份、内容不再互相污染」。
//!
//! 用户自建的实例**不做归属推断**：它既可能是任一壳建的，历史文件里也没有
//! 「谁建的」这个字段。两份文件初始都留着它，谁先改动谁的那份就分道扬镳。

use std::path::Path;

use crate::instance::{InstanceRegistry, RegistryError};
use crate::paths::ShellMode;

/// 另一个壳的默认实例 id（本壳的由调用方给）。
fn other_default_id(mode: ShellMode) -> &'static str {
    crate::instance::default_instance_id_for(match mode {
        ShellMode::Release => ShellMode::Dev,
        ShellMode::Dev => ShellMode::Release,
    })
}

/// setup 期调用一次：确保本壳拥有自己的注册表文件，且里面没有对方的默认实例
/// 记录。**只在真的要写盘时才写**——绝大多数启动是零 IO。
pub fn ensure_scoped(mode: ShellMode) -> Result<(), String> {
    let own = crate::paths::instances_registry_file_for(mode);
    if own.exists() {
        return release_foreign_record(mode, &own);
    }
    let shared = shared_legacy_file();
    let mut registry = if shared.exists() {
        read(&shared).unwrap_or_default()
    } else {
        InstanceRegistry::default()
    };
    drop_foreign_record(&mut registry, mode);
    crate::instance::save_registry_to(&registry, &own).map_err(|e| e.to_string())?;
    if shared.exists() && shared != own {
        eprintln!(
            "dsh-xlink: 实例注册表已按壳模式分家——本壳用 {}（release 用 {}）",
            own.display(),
            crate::paths::instances_registry_file_for(other_mode(mode)).display()
        );
    }
    Ok(())
}

/// 共享的旧注册表路径。release 认领之后它就是 release 自己的文件；dev 认领
/// 之后它仍然是 release 的历史来源，但**不再有 dev 写它**。
fn shared_legacy_file() -> std::path::PathBuf {
    crate::paths::instances_registry_file_for(ShellMode::Release)
}

fn other_mode(mode: ShellMode) -> ShellMode {
    match mode {
        ShellMode::Release => ShellMode::Dev,
        ShellMode::Dev => ShellMode::Release,
    }
}

/// 自己的文件已在，只需要判断「对方认领了没有」——认领了就把自己这份里的
/// 对方记录删掉。没认领就原样留着：对方启动时还要从它那里拷。
fn release_foreign_record(mode: ShellMode, own: &Path) -> Result<(), String> {
    let other_file = crate::paths::instances_registry_file_for(other_mode(mode));
    if !other_file.exists() {
        return Ok(());
    }
    let mut registry = read(own).map_err(|e| e.to_string())?;
    let foreign = other_default_id(mode);
    if registry.get(foreign).is_none() {
        return Ok(());
    }
    crate::instance::forget_default_pointer(&mut registry, foreign);
    registry.remove(foreign);
    crate::instance::save_registry_to(&registry, own).map_err(|e| e.to_string())
}

/// 删掉对方的默认实例记录。指针若指的是它，一并置空（那是本壳文件里的陈旧
/// 指针，置空后由 `instance::claim_default_pointer` 按本壳默认值重建）。
///
/// 指针那一半走 `instance::forget_default_pointer`——「谁在维护这个字段」只有
/// `instance.rs` 一处，`check:invariants` 第 12 项禁止本模块直接读它。
fn drop_foreign_record(registry: &mut InstanceRegistry, mode: ShellMode) {
    let foreign = other_default_id(mode);
    crate::instance::forget_default_pointer(registry, foreign);
    registry.remove(foreign);
}

fn read(path: &Path) -> Result<InstanceRegistry, RegistryError> {
    use crate::process::{read_state_file, StateRead};
    match read_state_file::<InstanceRegistry>(path) {
        StateRead::Loaded(value) if value.is_compatible() => Ok(value),
        StateRead::Loaded(value) => Err(RegistryError::IncompatibleSchema {
            found: value.schema_version,
            expected: crate::instance::CURRENT_REGISTRY_SCHEMA_VERSION,
        }),
        StateRead::Missing => Ok(InstanceRegistry::default()),
        StateRead::Corrupt { reason } => Err(RegistryError::Corrupt { reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::DEV_DEFAULT_INSTANCE_ID;
    use crate::tests::scoped_xlink_home;
    use std::path::PathBuf;

    fn temp_home(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "dsh-registry-split-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp home");
        dir
    }

    fn record(id: &str) -> crate::instance::InstanceRecord {
        crate::instance::InstanceRecord::new(
            id,
            crate::instance::KERNEL_FAMILY_DSH,
            3000 + id.len() as u16,
            1_700_000_000_000,
        )
    }

    /// 种一份拆分前的共享注册表，**返回写入的字节**——调用方要拿它比对
    /// 「dev 认领 / dev 写入之后 release 那份一个字节都没动」。
    fn seed_shared(home: &Path) -> Vec<u8> {
        let mut registry = InstanceRegistry::default();
        registry.add(record("default")).expect("add release");
        registry.add(record("default-dev")).expect("add dev");
        registry.add(record("scratch")).expect("add user-made");
        registry.default_instance_id = Some("default-dev".to_string());
        std::fs::create_dir_all(home.join("state")).unwrap();
        let text = serde_json::to_string_pretty(&registry).unwrap();
        std::fs::write(home.join("state/instances.json"), &text).unwrap();
        text.into_bytes()
    }

    /// 两个壳各写各的文件，互不影响：这是拆分的全部意义。
    #[test]
    fn each_shell_writes_only_its_own_registry_file() {
        let home = temp_home("scoped");
        let _guard = scoped_xlink_home(&home);
        let seeded = seed_shared(&home);

        ensure_scoped(ShellMode::Dev).expect("dev adopts");
        let dev_file = home.join("state/instances-dev.json");
        assert!(dev_file.is_file(), "dev 必须有自己的注册表文件");
        // release 的文件保持原样（release 还没启动过）。**比字节**：原先写成
        // `!exists() || true`，恒真、什么也没断言，等于给这条纪律留了个假哨兵。
        assert_eq!(
            std::fs::read(home.join("state/instances.json")).expect("release 文件仍在"),
            seeded,
            "dev 认领不得改动 release 那份共享注册表"
        );

        // dev 改自己的文件：release 那份一个字节都不许动。
        let shared_before = std::fs::read(home.join("state/instances.json")).unwrap();
        let mut dev = read(&dev_file).expect("read dev");
        dev.add(record("dev-only")).expect("add");
        crate::instance::save_registry_to(&dev, &dev_file).expect("save dev");
        assert_eq!(
            std::fs::read(home.join("state/instances.json")).unwrap(),
            shared_before,
            "dev 写自己的文件不得碰 release 的"
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// 认领不丢数据：用户自建的实例两边都还在，只有**能证明属于对方**的默认
    /// 实例会从自己这份里让位。
    #[test]
    fn adoption_keeps_user_made_instances_and_yields_the_other_default() {
        let home = temp_home("adopt");
        let _guard = scoped_xlink_home(&home);
        seed_shared(&home);

        ensure_scoped(ShellMode::Dev).expect("dev adopts");
        let dev = read(&home.join("state/instances-dev.json")).expect("read dev");
        assert!(dev.get("default-dev").is_some(), "dev 要认到自己的实例");
        assert!(
            dev.get("scratch").is_some(),
            "用户自建的实例不许在认领时丢失：归属无从推断，两边都先留着"
        );
        assert!(
            dev.get("default").is_none(),
            "release 的默认实例属于 release，不该出现在 dev 的列表里"
        );
        assert_eq!(
            dev.default_instance_id.as_deref(),
            Some(DEV_DEFAULT_INSTANCE_ID),
            "指针若指的是本壳的默认实例就沿用——它本来就是 dev 的；\
             指向对方实例的才置空（release 认领时就是这种情况，由 \
             instance::claim_default_pointer 改回 release 自己的）"
        );

        // release 之后启动：对方文件已存在 → 自己这份里的 default-dev 让位。
        ensure_scoped(ShellMode::Release).expect("release scopes");
        let rel = read(&home.join("state/instances.json")).expect("read release");
        assert!(rel.get("default").is_some(), "release 保住自己的实例");
        assert!(
            rel.get("scratch").is_some(),
            "用户自建的实例在 release 这边同样不许丢"
        );
        assert!(
            rel.get("default-dev").is_none(),
            "对方已认领，release 这份里的 default-dev 记录必须让位——\
             否则 release 的页签会列出并能启停 dev 的实例"
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// 幂等：反复跑不产生额外写入，也不改变内容。setup 每次启动都会调。
    #[test]
    fn scoping_is_idempotent() {
        let home = temp_home("idempotent");
        let _guard = scoped_xlink_home(&home);
        seed_shared(&home);
        ensure_scoped(ShellMode::Dev).expect("first");
        let dev_file = home.join("state/instances-dev.json");
        let first = std::fs::read(&dev_file).unwrap();
        ensure_scoped(ShellMode::Dev).expect("second");
        ensure_scoped(ShellMode::Dev).expect("third");
        assert_eq!(std::fs::read(&dev_file).unwrap(), first, "重复跑不得改内容");

        ensure_scoped(ShellMode::Release).expect("release first");
        let rel_before = std::fs::read(home.join("state/instances.json")).unwrap();
        ensure_scoped(ShellMode::Release).expect("release second");
        assert_eq!(
            std::fs::read(home.join("state/instances.json")).unwrap(),
            rel_before
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// release 先跑、dev 后跑也要收敛（顺序无关）。
    #[test]
    fn release_first_then_dev_converges() {
        let home = temp_home("order");
        let _guard = scoped_xlink_home(&home);
        seed_shared(&home);

        // release 先启动：dev 还没认领 → 这时**不能**把 default-dev 删掉。
        ensure_scoped(ShellMode::Release).expect("release first");
        let rel = read(&home.join("state/instances.json")).expect("read release");
        assert!(
            rel.get("default-dev").is_some(),
            "对方还没认领时不能删它的记录，否则对方永远读不到"
        );

        ensure_scoped(ShellMode::Dev).expect("dev adopts");
        ensure_scoped(ShellMode::Release).expect("release re-scopes");
        let rel = read(&home.join("state/instances.json")).expect("read release");
        assert!(rel.get("default-dev").is_none(), "对方认领后必须让位");
        let dev = read(&home.join("state/instances-dev.json")).expect("read dev");
        assert!(dev.get("default-dev").is_some(), "dev 的记录安然无恙");
        std::fs::remove_dir_all(&home).ok();
    }
}
