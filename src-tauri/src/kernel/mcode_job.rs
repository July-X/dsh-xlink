//! MiniMax-only Windows Job. No PID-based termination or shared DSH Job.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

pub(crate) struct McodeJob(OwnedHandle);

impl McodeJob {
    pub(crate) fn create() -> io::Result<Self> {
        // SAFETY: null selects an unnamed, non-inheritable Job with default security.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: ownership transfers to OwnedHandle exactly once.
        let owned = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: info has the exact documented layout and lives for the call.
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(owned))
    }

    pub(crate) fn assign(&self, child: &Child) -> io::Result<()> {
        // The child is a handshake-only launcher and cannot spawn until assigned.
        let job = self.0.as_raw_handle() as HANDLE;
        let process = child.as_raw_handle() as HANDLE;
        if unsafe { AssignProcessToJobObject(job, process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut in_job = 0;
        if unsafe { IsProcessInJob(process, job, &mut in_job) } == 0 || in_job == 0 {
            return Err(io::Error::other("无法确认 MiniMax 启动器已加入独立 Job"));
        }
        Ok(())
    }

    pub(crate) fn contains_pid(&self, pid: u32) -> io::Result<bool> {
        // A PID is only queried, never used to terminate a process.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut in_job = 0;
        let ok = unsafe { IsProcessInJob(handle, self.0.as_raw_handle() as HANDLE, &mut in_job) };
        let error = (ok == 0).then(io::Error::last_os_error);
        unsafe { CloseHandle(handle) };
        match error {
            Some(error) => Err(error),
            None => Ok(in_job != 0),
        }
    }

    pub(crate) fn active_count(&self) -> io::Result<u32> {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle() as HANDLE,
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(info.ActiveProcesses)
        }
    }

    pub(crate) fn wait_empty(&self, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.active_count()? == 0 {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other("等待 MiniMax 进程树退出超时"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub(crate) fn stop(&self) -> io::Result<()> {
        if self.active_count()? == 0 {
            return Ok(());
        }
        if unsafe { TerminateJobObject(self.0.as_raw_handle() as HANDLE, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        self.wait_empty(Duration::from_secs(5))
    }
}

// Windows file-system filters can briefly retain handles after all processes
// have exited. Retry only sharing/lock violations on our own test fixtures.
#[cfg(test)]
pub(crate) fn remove_fixture_directory(path: &std::path::Path) -> io::Result<()> {
    let temp = std::env::temp_dir().canonicalize()?;
    let target = path.canonicalize()?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if target.parent() != Some(temp.as_path())
        || !(name.starts_with("xlink-mcode-job-test-")
            || name.starts_with("dsh-xlink-adapter-mcode-"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a MiniMax test fixture",
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::remove_dir_all(&target) {
            Ok(()) => return Ok(()),
            Err(error)
                if matches!(error.raw_os_error(), Some(32 | 33)) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    struct Cleanup(Child);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn dedicated_job_tracks_real_descendants_and_leaves_unrelated_child_alone() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-job-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let fixture = home.join("fixture.js");
        let sentinel = home.join("started");
        let preload = home.join("preload.cjs");
        let preload_sentinel = home.join("preload-started");
        std::fs::write(&preload, "require('node:fs').writeFileSync(process.env.MCODE_JOB_TEST_PRELOAD_SENTINEL, 'started');").unwrap();
        std::fs::write(&fixture, r#"
          require('node:fs').writeFileSync(process.env.MCODE_JOB_TEST_SENTINEL, 'started');
          const {spawn} = require('node:child_process');
          const script = `const {spawn}=require('node:child_process');spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore',windowsHide:true});setInterval(()=>{},1000);`;
          spawn(process.execPath, ['-e', script], {stdio:'ignore',windowsHide:true});
          setInterval(()=>{},1000);
        "#).unwrap();
        let node = std::process::Command::new("where.exe")
            .arg("node.exe")
            .output()
            .expect("find Node");
        assert!(
            node.status.success(),
            "Node is required for the Windows Job integration test"
        );
        let node_path = String::from_utf8(node.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim()
            .to_owned();
        let mut command = Command::new(&node_path);
        command
            .arg("-e")
            .arg(crate::kernel::kernel_adapter::MCODE_WINDOWS_LAUNCHER)
            .arg(&fixture)
            .current_dir(&home)
            .env_remove("NODE_OPTIONS")
            .env(
                "DSH_XLINK_MCODE_NODE_OPTIONS",
                format!(
                    "--require=\"{}\"",
                    preload.to_string_lossy().replace('\\', "/")
                ),
            )
            .env("MCODE_JOB_TEST_SENTINEL", &sentinel)
            .env("MCODE_JOB_TEST_PRELOAD_SENTINEL", &preload_sentinel)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut launcher = Cleanup(crate::shell::process::quiet(&mut command).spawn().unwrap());
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !sentinel.exists(),
            "the service must not start before Job assignment"
        );
        assert!(
            !preload_sentinel.exists(),
            "user preloads must not run before Job assignment"
        );
        let job = McodeJob::create().unwrap();
        job.assign(&launcher.0).unwrap();
        assert!(job.contains_pid(launcher.0.id()).unwrap());
        let mut pipe = launcher.0.stdin.take().unwrap();
        pipe.write_all(&[1]).unwrap();
        drop(pipe);
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && (!sentinel.exists() || job.active_count().unwrap() < 4) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(sentinel.exists(), "the owned service did not start");
        assert!(
            preload_sentinel.exists(),
            "the service must receive the sanitized Node preload"
        );
        assert!(
            job.active_count().unwrap() >= 4,
            "launcher, service, child and grandchild must all be in the Job"
        );
        let mut other_command = Command::new(&node_path);
        other_command
            .arg("-e")
            .arg("setInterval(()=>{},1000)")
            .current_dir(&home)
            .env_remove("NODE_OPTIONS")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut unrelated = Cleanup(
            crate::shell::process::quiet(&mut other_command)
                .spawn()
                .unwrap(),
        );
        assert!(!job.contains_pid(unrelated.0.id()).unwrap());
        job.stop().unwrap();
        assert_eq!(job.active_count().unwrap(), 0);
        assert!(
            unrelated.0.try_wait().unwrap().is_none(),
            "unrelated process must not be terminated"
        );
        drop(unrelated);
        drop(launcher);
        drop(job);
        drop(guard);
        remove_fixture_directory(&home).unwrap();
    }

    #[test]
    fn fixture_cleanup_waits_for_windows_sharing_locks() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-job-test-cleanup-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&home).unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(home.join("locked"))
            .unwrap();
        let closer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(file);
        });
        let result = remove_fixture_directory(&home);
        closer.join().unwrap();
        result.unwrap();
        assert!(!home.exists());
    }
}
