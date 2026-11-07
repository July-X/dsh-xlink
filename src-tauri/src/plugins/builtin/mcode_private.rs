//! Windows-only MiniMax credential writer. The private DACL exists before bytes are written.

use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
#[cfg(test)]
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, MoveFileExW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, MOVEFILE_WRITE_THROUGH,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct LocalMemory(*mut c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn current_user_sid() -> io::Result<String> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle(token);
        let mut needed = 0;
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed);
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let memory = LocalMemory(text.cast());
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        drop(memory);
        Ok(result)
    }
}

pub(super) fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let sid = current_user_sid()?;
    let sddl = format!("D:P(A;;FA;;;{sid})");
    let mut descriptor = null_mut();
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(sddl.as_ref()).as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    let descriptor = LocalMemory(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("credential has no parent"))?;
    let temporary = parent.join(format!(
        ".xlink-memory-{}-{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    let handle = unsafe {
        CreateFileW(
            wide(temporary.as_os_str()).as_ptr(),
            GENERIC_WRITE,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut file = unsafe { File::from_raw_handle(handle) };
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        if unsafe {
            MoveFileExW(
                wide(temporary.as_os_str()).as_ptr(),
                wide(path.as_os_str()).as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertStringSidToSidW,
        GetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        AccessCheck, CreateRestrictedToken, DuplicateTokenEx, EqualSid, GetAce,
        SecurityImpersonation, TokenImpersonation, ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION,
        GENERIC_MAPPING, GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PRIVILEGE_SET,
        SID_AND_ATTRIBUTES, TOKEN_DUPLICATE, TOKEN_IMPERSONATE,
    };

    #[test]
    fn credential_dacl_is_private_at_creation_and_rejects_restricted_token() {
        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-acl-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("credential.json");
        write_private(&path, b"test credential").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"test credential");
        assert!(write_private(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"test credential");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        unsafe {
            let file = File::open(&path).unwrap();
            let mut descriptor = null_mut();
            let mut dacl = null_mut();
            assert_eq!(
                GetSecurityInfo(
                    file.as_raw_handle(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION
                        | GROUP_SECURITY_INFORMATION
                        | DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    &mut dacl,
                    null_mut(),
                    &mut descriptor
                ),
                0
            );
            let descriptor = LocalMemory(descriptor);
            let mut text = null_mut();
            assert_ne!(
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor.0,
                    SDDL_REVISION_1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    null_mut()
                ),
                0
            );
            let text_memory = LocalMemory(text.cast());
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let sddl = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            assert!(sddl.starts_with("D:P"));
            assert_eq!(sddl.matches('(').count(), 1);
            // SDDL 对内建 Administrator 会输出别名「LA」，windows-2025 runner 正以该账户执行：
            // 拿原始 SID 文本比对会在 CI 上假红，这里改比 ACE 里的 SID 二进制。
            let mut expected = null_mut();
            assert_ne!(
                ConvertStringSidToSidW(
                    wide(current_user_sid().unwrap().as_ref()).as_ptr(),
                    &mut expected
                ),
                0
            );
            let expected = LocalMemory(expected);
            let mut ace = null_mut();
            assert_ne!(GetAce(dacl, 0, &mut ace), 0);
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            // windows-sys 没有导出 ACCESS_ALLOWED_ACE_TYPE，它的值就是 0。
            assert_eq!(ace.Header.AceType, 0, "唯一的 ACE 必须是允许类型");
            assert_ne!(
                EqualSid(
                    std::ptr::addr_of!(ace.SidStart).cast_mut().cast(),
                    expected.0
                ),
                0
            );
            assert!(!sddl.contains(";ID;"));
            drop(text_memory);

            let mut token = null_mut();
            assert_ne!(
                OpenProcessToken(
                    GetCurrentProcess(),
                    TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE,
                    &mut token
                ),
                0
            );
            let token = OwnedHandle(token);
            let mut impersonation = null_mut();
            assert_ne!(
                DuplicateTokenEx(
                    token.0,
                    TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE,
                    null(),
                    SecurityImpersonation,
                    TokenImpersonation,
                    &mut impersonation
                ),
                0
            );
            let impersonation = OwnedHandle(impersonation);
            let mut sid = null_mut();
            assert_ne!(
                ConvertStringSidToSidW(wide("S-1-5-7".as_ref()).as_ptr(), &mut sid),
                0
            );
            let sid = LocalMemory(sid);
            let restricting = SID_AND_ATTRIBUTES {
                Sid: sid.0,
                Attributes: 0,
            };
            let mut restricted = null_mut();
            assert_ne!(
                CreateRestrictedToken(
                    impersonation.0,
                    0,
                    0,
                    null(),
                    0,
                    null(),
                    1,
                    &restricting,
                    &mut restricted
                ),
                0
            );
            let restricted = OwnedHandle(restricted);
            for (candidate, expected) in [(impersonation.0, 1), (restricted.0, 0)] {
                let mapping = GENERIC_MAPPING {
                    GenericRead: 0x120089,
                    GenericWrite: 0x120116,
                    GenericExecute: 0x1200a0,
                    GenericAll: 0x1f01ff,
                };
                let mut privileges = [0usize; 128];
                let mut length = std::mem::size_of_val(&privileges) as u32;
                let mut granted = 0;
                let mut allowed = -1;
                assert_ne!(
                    AccessCheck(
                        descriptor.0,
                        candidate,
                        1,
                        &mapping,
                        privileges.as_mut_ptr().cast::<PRIVILEGE_SET>(),
                        &mut length,
                        &mut granted,
                        &mut allowed
                    ),
                    0
                );
                assert_eq!(allowed, expected);
            }
            drop(file);
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
