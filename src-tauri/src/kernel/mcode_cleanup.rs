//! 残留回收只经本轮启动器的随机凭据；旧 PID、端口与进程名都不是终止权限。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Receipt {
    version: u32,
    pub(crate) pid: u32,
    pub(crate) nonce: String,
    pub(crate) endpoint: String,
    endpoint_id: String,
    node: PathBuf,
    ports: [u16; 2],
}

#[derive(Deserialize)]
pub(crate) struct Owner {
    nonce: String,
    pub(crate) pid: u32,
    pub(crate) service_pid: Option<u32>,
    pub(crate) parent_connected: bool,
    accepted: bool,
    ports: [u16; 2],
}

impl Receipt {
    pub(crate) fn new(node: &Path, port: u16, miniapp_port: u16) -> Self {
        let nonce = format!(
            "{:032x}{:032x}",
            rand::random::<u128>(),
            rand::random::<u128>()
        );
        let endpoint_id = format!("{:032x}", rand::random::<u128>());
        #[cfg(windows)]
        let endpoint = format!(r"\\.\pipe\xlink-mcode-{endpoint_id}");
        #[cfg(not(windows))]
        let endpoint = format!("/tmp/xlink-mcode-{endpoint_id}.sock");
        Self {
            version: 2,
            pid: 0,
            nonce,
            endpoint,
            endpoint_id,
            node: node.to_path_buf(),
            ports: [port, miniapp_port],
        }
    }

    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        crate::shell::atomic_write::atomic_write_private(path, &bytes)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn load(
        path: &Path,
        pid: u32,
        ports: [u16; 2],
        node: &Path,
    ) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|_| {
            "旧版本未保存可核验的回收凭据，不能仅凭 PID 或端口结束进程；请先确认残留归属并手动停止，再点「仅清理旧记录」".to_string()
        })?;
        // 先读取版本头：旧版没有独立端点标识，不能误报成损坏，也不能继续使用。
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| "回收凭据损坏；未结束任何进程，请手动核查后重试".to_string())?;
        match value.get("version").and_then(serde_json::Value::as_u64) {
            Some(1) => return Err("这是旧版回收凭据（版本 1），端点与认证密钥未分离，已拒绝自动清理；请核对归属并手动停止残留，再点「仅清理旧记录」。重新启动后会生成新版凭据".into()),
            Some(2) => {}
            Some(_) => return Err("回收凭据版本不受支持；未结束任何进程，请使用匹配版本的应用或手动核查残留".into()),
            None => return Err("回收凭据缺少有效版本；未结束任何进程，请手动核查残留".into()),
        }
        let record: Self = serde_json::from_value(value)
            .map_err(|_| "新版回收凭据字段损坏；未结束任何进程，请手动核查后重试".to_string())?;
        if record.version != 2
            || record.pid != pid
            || record.ports != ports
            || record.node != node
            || record.endpoint_id.len() != 32
            || !record
                .endpoint_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || record.nonce.len() != 64
            || !record.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(
                "回收凭据与当前实例、进程或端口不一致，未结束任何进程；请刷新并检查运行环境".into(),
            );
        }
        // 公开端点标识与认证密钥独立；不能从可枚举的管道/文件名推回密钥。
        #[cfg(windows)]
        let endpoint = format!(r"\\.\pipe\xlink-mcode-{}", record.endpoint_id);
        #[cfg(not(windows))]
        let endpoint = format!("/tmp/xlink-mcode-{}.sock", record.endpoint_id);
        if record.endpoint != endpoint {
            return Err("回收端点与本轮凭据不一致，未结束任何进程".into());
        }
        Ok(record)
    }

    pub(crate) fn request(&self, path: &Path, action: &str) -> Result<Owner, String> {
        let mut command = crate::shell::process::command_with_path(&self.node);
        // 回收助手不执行任何用户预加载，避免凭据被无关代码读取。
        command
            .env_remove("NODE_OPTIONS")
            .env("DSH_XLINK_CLEANUP_EXPECTED", &self.nonce)
            .env("DSH_XLINK_CLEANUP_ENDPOINT", &self.endpoint)
            .arg("-e")
            .arg(include_str!("mcode-cleanup-client.cjs"))
            .arg(path)
            .arg(action);
        let (ok, output, _) =
            crate::shell::process::run_command_capture(command, "MiniMax 残留归属核验").map_err(
                |_| "无法连接原启动器，未按 PID 或端口结束进程；请手动核查残留".to_string(),
            )?;
        if !ok {
            return Err(
                "原启动器已退出或无法验证回收凭据；未按 PID 或端口结束进程，请手动核查残留".into(),
            );
        }
        let owner: Owner = serde_json::from_str(&output)
            .map_err(|_| "原启动器未返回可验证的身份；请手动核查残留".to_string())?;
        self.verify(&owner)?;
        if action == "stop" && !owner.accepted {
            return Err(
                "该实例仍由另一个运行窗口管理，请在那个窗口停止服务；本次未结束进程".into(),
            );
        }
        Ok(owner)
    }

    #[cfg(windows)]
    pub(crate) fn wait_ready(&self, _: &Path) -> Result<(), String> {
        // Windows 暂不开放残留控制管道；正常启动仍由已有 Job 与 stdin 管理。
        Ok(())
    }

    #[cfg(not(windows))]
    pub(crate) fn wait_ready(&self, path: &Path) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match self.request(path, "probe") {
                Ok(_) => return Ok(()),
                Err(error) if std::time::Instant::now() >= deadline => return Err(error),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
    }

    fn verify(&self, owner: &Owner) -> Result<(), String> {
        if owner.nonce != self.nonce || owner.pid != self.pid || owner.ports != self.ports {
            Err("进程身份已改变或 PID 被复用，拒绝清理；请刷新后检查端口归属".into())
        } else {
            Ok(())
        }
    }
}

pub(crate) fn require_supported_platform() -> Result<(), String> {
    if cfg!(windows) {
        Err("Windows 暂未启用可验证访问权限的残留进程清理；未结束任何进程。请在任务管理器确认归属并结束残留，再点「仅清理旧记录」".into())
    } else {
        Ok(())
    }
}

/// 提示先查监听者再查实例归属；旧 PID 只作参考，不能变成“结束所有 node”。
pub(crate) fn manual_check_hint(pid: u32, port: u16, miniapp_port: u16) -> String {
    if cfg!(windows) {
        format!("手动检查：① 打开“资源监视器 → 网络 → 侦听端口”，查看 {port} 和 {miniapp_port} 的当前 PID（旧记录 {pid} 仅作参考）。② 在“任务管理器 → 详细信息”启用“命令行”列，核对该 PID 的 Node/MiniMax 程序路径及实例归属；若属于另一运行窗口，请到那个窗口停止。③ 仅在确认是本实例残留后结束对应进程；无法确认就不要结束。回到概览刷新，两个端口释放后点“仅清理旧记录”，再启动。")
    } else if cfg!(target_os = "macos") {
        format!("手动检查：① 在终端分别运行只读命令 lsof -nP -iTCP:{port} -sTCP:LISTEN 和 lsof -nP -iTCP:{miniapp_port} -sTCP:LISTEN，查看当前监听 PID（旧记录 {pid} 仅作参考）。② 在“活动监视器”搜索该 PID，双击查看程序路径、父进程和“打开的文件和端口”，核对 MiniMax 及实例目录；若属于另一窗口，请到那个窗口停止。③ 仅在确认是本实例残留后先选“退出”，无效时再选“强制退出”；无法确认就不要结束。回到概览刷新，端口释放后点“仅清理旧记录”，再启动。")
    } else {
        format!("手动检查：用只读命令 ss -lptn 查看 {port} 和 {miniapp_port} 的监听 PID，再用 ps -fp <PID> 核对程序路径和实例归属。旧记录 {pid} 仅作参考，不能直接据此结束进程；无法确认就不要结束。确认本实例残留并手动停止后，刷新概览，端口释放后点“仅清理旧记录”，再启动。")
    }
}

/// 空端口无需回收；占用但读不到 PID 时必须拒绝，不把未知当成自己。
pub(crate) fn listener_owned(available: bool, listener: Option<u32>, service: Option<u32>) -> bool {
    available || (service.is_some() && listener == service)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_and_future_receipts_have_explicit_version_errors() {
        let dir = std::env::temp_dir().join(format!(
            "xlink-cleanup-version-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("receipt.json");
        let node = Path::new("/node");
        let mut value = serde_json::to_value(Receipt::new(node, 4001, 4002)).unwrap();
        assert_eq!(value["version"], 2);
        value["version"] = 1.into();
        value.as_object_mut().unwrap().remove("endpoint_id");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = Receipt::load(&path, 42, [4001, 4002], node).err().unwrap();
        assert!(error.contains("旧版回收凭据"));
        assert!(!error.contains("损坏"));
        value["version"] = 99.into();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(Receipt::load(&path, 42, [4001, 4002], node)
            .err()
            .unwrap()
            .contains("版本不受支持"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn manual_guidance_identifies_both_ports_and_requires_ownership_checks() {
        let hint = manual_check_hint(42, 4001, 4002);
        for text in [
            "4001",
            "4002",
            "42",
            "归属",
            "无法确认就不要结束",
            "仅清理旧记录",
        ] {
            assert!(hint.contains(text));
        }
        if cfg!(windows) {
            assert!(hint.contains("资源监视器"));
        } else if cfg!(target_os = "macos") {
            assert!(hint.contains("lsof -nP"));
        } else {
            assert!(hint.contains("ss -lptn"));
        }
    }

    #[test]
    fn public_endpoint_identifier_is_independent_of_authentication_secret() {
        let receipt = Receipt::new(Path::new("/node"), 4001, 4002);
        assert_eq!(receipt.endpoint_id.len(), 32);
        assert_eq!(receipt.nonce.len(), 64);
        assert!(receipt.endpoint.contains(&receipt.endpoint_id));
        assert!(!receipt.endpoint.contains(&receipt.nonce));
        assert_ne!(receipt.endpoint_id, receipt.nonce[..32]);
        assert_eq!(require_supported_platform().is_err(), cfg!(windows));
    }

    #[test]
    fn only_free_or_authenticated_service_ports_are_allowed() {
        assert!(listener_owned(true, None, None));
        assert!(listener_owned(false, Some(43), Some(43)));
        assert!(!listener_owned(false, Some(44), Some(43)));
        assert!(!listener_owned(false, None, Some(43)));
        assert!(!listener_owned(false, None, None));
        assert!(!listener_owned(false, Some(43), None));
    }

    #[test]
    fn missing_malformed_and_replaced_receipts_refuse_without_side_effects() {
        let dir = std::env::temp_dir().join(format!(
            "xlink-cleanup-receipt-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("receipt.json");
        let node = Path::new("/fixture/node");
        assert!(Receipt::load(&path, 42, [4001, 4002], node).is_err());
        std::fs::write(&path, b"broken").unwrap();
        assert!(Receipt::load(&path, 42, [4001, 4002], node).is_err());
        let mut receipt = Receipt::new(node, 4001, 4002);
        receipt.pid = 42;
        receipt.save(&path).unwrap();
        assert!(Receipt::load(&path, 42, [4001, 4002], node).is_ok());
        assert!(Receipt::load(&path, 43, [4001, 4002], node).is_err());
        assert!(Receipt::load(&path, 42, [4001, 4003], node).is_err());
        assert!(Receipt::load(&path, 42, [4001, 4002], Path::new("/other/node")).is_err());
        receipt.endpoint = "remote-host".into();
        receipt.save(&path).unwrap();
        assert!(Receipt::load(&path, 42, [4001, 4002], node).is_err());
        assert!(path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn changed_generation_pid_or_ports_never_authorize_cleanup() {
        let mut receipt = Receipt::new(Path::new("/node"), 4001, 4002);
        receipt.pid = 42;
        let mut owner = Owner {
            nonce: receipt.nonce.clone(),
            pid: 42,
            service_pid: Some(43),
            parent_connected: false,
            accepted: true,
            ports: [4001, 4002],
        };
        assert!(receipt.verify(&owner).is_ok());
        owner.pid = 44;
        assert!(receipt.verify(&owner).is_err());
        owner.pid = 42;
        owner.nonce = "0".repeat(64);
        assert!(receipt.verify(&owner).is_err());
        owner.nonce = receipt.nonce.clone();
        owner.ports.swap(0, 1);
        assert!(receipt.verify(&owner).is_err());
    }
}
