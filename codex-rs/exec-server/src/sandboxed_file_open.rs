use codex_exec_server_protocol::JSONRPCErrorError;
use codex_sandboxing::SandboxExecRequest;
use codex_utils_path_uri::PathUri;
use tokio::io;

use crate::file_system_mutation::EffectGuard;
use crate::fs_helper::FsHelperOpenParams;
use crate::fs_helper::FsHelperOpenResponse;
use crate::fs_helper::FsHelperPayload;
use crate::fs_helper::FsHelperRequest;
use crate::fs_helper::FsHelperResponse;
#[cfg(unix)]
use crate::fs_sandbox::check_helper_output;
#[cfg(unix)]
use crate::fs_sandbox::collect_helper_output;
#[cfg(windows)]
use crate::fs_sandbox::drain_helper_stderr;
#[cfg(windows)]
use crate::fs_sandbox::finish_helper;
use crate::fs_sandbox::io_error;
#[cfg(windows)]
use crate::fs_sandbox::read_helper_response;
use crate::fs_sandbox::spawn_command;
use crate::protocol::FsOpenMode;
use crate::rpc::internal_error;
use crate::rpc::invalid_request;

pub(crate) async fn open(
    command: SandboxExecRequest,
    path: PathUri,
    mode: FsOpenMode,
) -> Result<tokio::fs::File, JSONRPCErrorError> {
    let request = serde_json::to_vec(&FsHelperRequest::Open(FsHelperOpenParams { path, mode }))
        .map_err(|error| internal_error(format!("invalid fs sandbox helper request: {error}")))?;
    open_platform(command, request, mode).await
}

fn open_response(
    response: &[u8],
    effect: &mut Option<EffectGuard>,
) -> Result<FsHelperOpenResponse, JSONRPCErrorError> {
    match serde_json::from_slice(response).map_err(|error| {
        internal_error(format!("invalid fs sandbox helper open response: {error}"))
    })? {
        FsHelperResponse::Ok(FsHelperPayload::Open(response)) => {
            if let Some(effect) = effect {
                effect.settle();
            }
            Ok(response)
        }
        FsHelperResponse::Ok(_) => Err(invalid_request(
            "invalid fs sandbox helper open response".to_string(),
        )),
        FsHelperResponse::Error(error) => {
            if let Some(effect) = effect {
                effect.settle();
            }
            Err(error)
        }
    }
}

// Unix passes the opened fd over the helper's stdin socket.
#[cfg(unix)]
async fn open_platform(
    command: SandboxExecRequest,
    request: Vec<u8>,
    mode: FsOpenMode,
) -> Result<tokio::fs::File, JSONRPCErrorError> {
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    let (mut receiver, sender) = UnixStream::pair().map_err(io_error)?;
    let sender: OwnedFd = sender.into();
    let mut child = spawn_command(command, codex_utils_pty::ChildStdin::File(sender))?;
    let mut effect = (mode == FsOpenMode::Replace).then(EffectGuard::new);
    let sent = receiver
        .write_all(&request)
        .and_then(|()| receiver.shutdown(std::net::Shutdown::Write));
    // A failed send still waits for the helper, retaining ownership of truncation.
    let (exited, output) = collect_helper_output(&mut child).await;
    if exited && let Some(effect) = &mut effect {
        effect.settle();
    }
    sent.map_err(io_error)?;
    let output = check_helper_output(output)?;
    open_response(&output.stdout, &mut effect)?;
    let descriptor = receive_file_descriptor(&receiver).map_err(io_error)?;
    Ok(tokio::fs::File::from_std(std::fs::File::from(descriptor)))
}

// Windows duplicates the helper's handle before letting it exit.
#[cfg(windows)]
async fn open_platform(
    command: SandboxExecRequest,
    mut request: Vec<u8>,
    mode: FsOpenMode,
) -> Result<tokio::fs::File, JSONRPCErrorError> {
    use tokio::io::AsyncWriteExt;

    let mut child = spawn_command(command, codex_utils_pty::ChildStdin::Piped)?;
    let mut effect = (mode == FsOpenMode::Replace).then(EffectGuard::new);
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = drain_helper_stderr(&mut child);
    request.push(b'\n');
    let result = async {
        let stdin = stdin
            .as_mut()
            .ok_or_else(|| internal_error("missing fs sandbox helper stdin".to_string()))?;
        stdin.write_all(&request).await.map_err(io_error)?;
        stdin.flush().await.map_err(io_error)?;
        let stdout =
            stdout.ok_or_else(|| internal_error("missing fs sandbox helper stdout".to_string()))?;
        let response = read_helper_response(stdout).await?;
        // A valid terminal response settles truncation even if handle duplication fails.
        let response = open_response(&response, &mut effect)?;
        duplicate_file_handle(response.process_id, response.file_handle).map_err(io_error)
    }
    .await;
    drop(stdin);
    let (exited, cleanup) = finish_helper(&mut child, stderr).await;
    if exited && let Some(effect) = &mut effect {
        effect.settle();
    }
    cleanup?;
    result.map(tokio::fs::File::from_std)
}

// SCM_RIGHTS is Unix-only.
#[cfg(unix)]
pub(crate) fn transfer_file(file: &tokio::fs::File) -> io::Result<()> {
    use rustix::net::SendAncillaryBuffer;
    use rustix::net::SendAncillaryMessage;
    use rustix::net::SendFlags;
    use std::io::IoSlice;
    use std::os::fd::AsFd;

    let descriptors = [file.as_fd()];
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&descriptors)) {
        return Err(io::Error::other("missing file-descriptor control header"));
    }
    if rustix::net::sendmsg(
        std::io::stdin(),
        &[IoSlice::new(&[0])],
        &mut control,
        SendFlags::empty(),
    )? != 1
    {
        return Err(io::Error::other(
            "fs sandbox helper did not transfer its opened file descriptor",
        ));
    }
    Ok(())
}

// File-descriptor passing is only available on Unix.
#[cfg(unix)]
fn receive_file_descriptor(
    socket: &std::os::unix::net::UnixStream,
) -> io::Result<std::os::fd::OwnedFd> {
    use rustix::net::RecvAncillaryBuffer;
    use rustix::net::RecvAncillaryMessage;
    use rustix::net::RecvFlags;
    use rustix::net::ReturnFlags;
    use std::io::IoSliceMut;

    let mut byte = [0_u8];
    let mut buffers = [IoSliceMut::new(&mut byte)];
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    // Linux can set close-on-exec while receiving the fd.
    #[cfg(target_os = "linux")]
    let flags = RecvFlags::CMSG_CLOEXEC;
    // Other Unix platforms need the non-atomic fcntl call below.
    #[cfg(not(target_os = "linux"))]
    let flags = RecvFlags::empty();
    let message = rustix::net::recvmsg(socket, &mut buffers, &mut control, flags)?;
    if message.bytes != 1 || message.flags.contains(ReturnFlags::CTRUNC) {
        return Err(io::Error::other("invalid file-descriptor control message"));
    }
    let descriptor = control
        .drain()
        .find_map(|message| match message {
            RecvAncillaryMessage::ScmRights(mut descriptors) => descriptors.next(),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("missing transferred file descriptor"))?;
    // macOS cannot set this atomically, so the fd is briefly inheritable.
    // Shell and filesystem helper launches close inherited fds to limit that race.
    #[cfg(not(target_os = "linux"))]
    rustix::io::fcntl_setfd(&descriptor, rustix::io::FdFlags::CLOEXEC)?;
    Ok(descriptor)
}

// Windows file handles must be duplicated across processes.
#[cfg(windows)]
fn duplicate_file_handle(process_id: u32, file_handle: u64) -> io::Result<std::fs::File> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::io::FromRawHandle;
    use std::os::windows::io::OwnedHandle;
    use windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS;
    use windows_sys::Win32::Foundation::DuplicateHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    use windows_sys::Win32::System::Threading::OpenProcess;
    use windows_sys::Win32::System::Threading::PROCESS_DUP_HANDLE;

    // SAFETY: OpenProcess returns an owned handle or null on failure.
    let process = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, process_id) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The successful OpenProcess result is owned by this scope.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut duplicated = std::ptr::null_mut();
    // SAFETY: Both process handles remain valid and duplicated receives an owned file handle.
    if unsafe {
        DuplicateHandle(
            process.as_raw_handle(),
            file_handle as HANDLE,
            GetCurrentProcess(),
            &raw mut duplicated,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: DuplicateHandle transferred ownership of the new file handle.
    Ok(unsafe { std::fs::File::from_raw_handle(duplicated) })
}
