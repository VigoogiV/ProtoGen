//! 系统文件选择对话框。
//!
//! 为什么要做这个：Windows 的 conhost 控制台**不支持**把文件拖进窗口
//! （只有 Windows Terminal 会插入路径），所以"拖拽"这条路靠不住。
//! 双击启动的用户按回车弹系统对话框，才是真正可用的。
//!
//! 用 Win32 的 `GetOpenFileNameW`（comdlg32），不引入额外 crate。

/// 弹出"打开文件"对话框，返回选中的路径；用户取消返回 None
#[cfg(windows)]
pub fn pick_proto_file() -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    let mut buf = vec![0u16; 1024];

    // 过滤器：`proto 文件\0*.proto\0所有文件\0*.*\0\0`
    let filter: Vec<u16> = "proto 文件\0*.proto\0所有文件\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "选择 .proto 文件\0".encode_utf16().collect();

    let mut ofn: OpenFileNameW = unsafe { std::mem::zeroed() };
    ofn.l_struct_size = std::mem::size_of::<OpenFileNameW>() as u32;
    ofn.lpstr_filter = filter.as_ptr();
    ofn.lpstr_file = buf.as_mut_ptr();
    ofn.n_max_file = buf.len() as u32;
    ofn.lpstr_title = title.as_ptr();
    ofn.lpstr_def_ext = std::ptr::null();
    // OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR
    ofn.flags = 0x0008_0000 | 0x0000_1000 | 0x0000_0800 | 0x0000_0008;

    let ok = unsafe { GetOpenFileNameW(&mut ofn) };
    if ok == 0 {
        return None;
    }

    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let os = std::ffi::OsString::from_wide(&buf[..len]);
    let path = std::path::PathBuf::from(os);
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(not(windows))]
pub fn pick_proto_file() -> Option<std::path::PathBuf> {
    None
}

/// 当前平台是否支持弹出对话框
pub const HAS_DIALOG: bool = cfg!(windows);

#[cfg(windows)]
#[repr(C)]
struct OpenFileNameW {
    l_struct_size: u32,
    hwnd_owner: *mut core::ffi::c_void,
    h_instance: *mut core::ffi::c_void,
    lpstr_filter: *const u16,
    lpstr_custom_filter: *mut u16,
    n_max_cust_filter: u32,
    n_filter_index: u32,
    lpstr_file: *mut u16,
    n_max_file: u32,
    lpstr_file_title: *mut u16,
    n_max_file_title: u32,
    lpstr_initial_dir: *const u16,
    lpstr_title: *const u16,
    flags: u32,
    n_file_offset: u16,
    n_file_extension: u16,
    lpstr_def_ext: *const u16,
    l_cust_data: isize,
    lpfn_hook: *mut core::ffi::c_void,
    lp_template_name: *const u16,
    pv_reserved: *mut core::ffi::c_void,
    dw_reserved: u32,
    flags_ex: u32,
}

#[cfg(windows)]
#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileNameW) -> i32;
}
