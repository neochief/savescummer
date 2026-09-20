use super::*;
use std::{ffi::OsString, os::windows::ffi::OsStringExt};
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    System::Threading::*,
};

pub fn user_id() -> io::Result<String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut bytes = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut bytes);
    }
    // Pointer-aligned backing allocation for TOKEN_USER and its SID.
    let mut buffer = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    };
    unsafe {
        CloseHandle(token);
    }
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
    let mut sid = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0;
    unsafe {
        while *sid.add(len) != 0 {
            len += 1;
        }
    }
    let value = OsString::from_wide(unsafe { std::slice::from_raw_parts(sid, len) })
        .to_string_lossy()
        .into_owned();
    unsafe {
        LocalFree(sid.cast());
    }
    Ok(value)
}
fn create(path: &Path, first: bool) -> io::Result<NamedPipeServer> {
    let sddl = format!("D:P(A;;GA;;;{})", user_id()?);
    let sddl = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let result = unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                path,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            )
    };
    unsafe {
        LocalFree(descriptor);
    }
    result
}
pub struct Listener {
    next: NamedPipeServer,
    path: PathBuf,
}
impl Listener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        Ok(Self {
            next: create(path, true)?,
            path: path.to_path_buf(),
        })
    }
    pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        self.next.connect().await?;
        let next = create(&self.path, false)?;
        Ok(std::mem::replace(&mut self.next, next))
    }
}
pub async fn connect(path: &Path) -> io::Result<NamedPipeClient> {
    for attempt in 0..50 {
        match ClientOptions::new().open(path) {
            Ok(client) => return Ok(client),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && attempt < 49 => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}
