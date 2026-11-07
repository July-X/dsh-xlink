//! The management view selection does not change a running instance.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::shell::{paths, process, settings};

static SELECTION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum KernelSelection {
    Dsh,
    Mcode,
}

fn read(mode: paths::ShellMode) -> Result<KernelSelection, String> {
    let path = paths::shell_dir(mode).join("kernel-selection.json");
    match process::read_state_file(&path) {
        process::StateRead::Loaded(selection) => Ok(selection),
        process::StateRead::Missing => Ok(KernelSelection::Dsh),
        process::StateRead::Corrupt { reason } => Err(format!(
            "无法读取当前工作台选择：{reason}（文件：{}）。请删除该文件，或重新选择内核以覆盖它",
            path.display()
        )),
    }
}

/// 写当前选择。**文件损坏时也照写**：它只有两个取值，写出去的是完整枚举值，
/// 覆盖一个解析失败的文件不会丢信息；反过来拒写会让内核族页签永久卡死。
fn write(mode: paths::ShellMode, selection: KernelSelection) -> Result<(), String> {
    let _guard = crate::lock(&SELECTION_LOCK);
    let dir = paths::shell_dir(mode);
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建壳设置目录失败：{error}"))?;
    let path = dir.join("kernel-selection.json");
    let mut bytes = serde_json::to_vec(&selection).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    process::atomic_write(&path, &bytes)
        .map_err(|error| format!("保存当前工作台选择失败：{error}；文件：{}", path.display()))
}

#[tauri::command]
pub(crate) async fn get_kernel_selection() -> Result<KernelSelection, String> {
    crate::commands::blocking(|| read(settings::current_mode())).await
}

#[tauri::command]
pub(crate) async fn set_kernel_selection(selection: KernelSelection) -> Result<(), String> {
    crate::commands::blocking(move || write(settings::current_mode(), selection)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_scoped_to_shell_and_preserves_corrupt_state() {
        let home =
            std::env::temp_dir().join(format!("xlink-selection-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        assert_eq!(read(paths::ShellMode::Dev).unwrap(), KernelSelection::Dsh);
        assert!(!paths::shell_dir(paths::ShellMode::Dev).exists());
        assert!(serde_json::from_str::<KernelSelection>("\"unknown\"").is_err());
        write(paths::ShellMode::Dev, KernelSelection::Mcode).unwrap();
        assert_eq!(read(paths::ShellMode::Dev).unwrap(), KernelSelection::Mcode);
        assert_eq!(
            read(paths::ShellMode::Release).unwrap(),
            KernelSelection::Dsh
        );
        write(paths::ShellMode::Release, KernelSelection::Dsh).unwrap();
        assert_eq!(read(paths::ShellMode::Dev).unwrap(), KernelSelection::Mcode);
        let path = paths::shell_dir(paths::ShellMode::Dev).join("kernel-selection.json");
        std::fs::write(&path, b"invalid").unwrap();
        let error = read(paths::ShellMode::Dev).unwrap_err();
        assert!(error.contains(&path.display().to_string()), "{error}");
        // 损坏的文件可以直接被合法枚举值覆盖：它只有两个取值，覆盖不丢信息，
        // 拒写会让内核族页签永久卡死。
        write(paths::ShellMode::Dev, KernelSelection::Dsh).unwrap();
        assert_eq!(read(paths::ShellMode::Dev).unwrap(), KernelSelection::Dsh);
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }
}
