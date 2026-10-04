//! fbdevの装置。LinuxのfbdevのABI(`linux/fb.h`)で、画面の情報を読み、画面の領域を対応づける。

use std::{
    fs::{File, OpenOptions},
    io,
    os::fd::{AsFd, AsRawFd, BorrowedFd},
    path::Path,
    ptr::NonNull,
};

use seinas_render::FrameView;

use crate::{
    layout::{Bitfield, Layout, ScreenInfo},
    present::Presenter,
    FbError,
};

/// 変えられる画面の情報を読む(`FBIOGET_VSCREENINFO`)。
const FBIOGET_VSCREENINFO: u32 = 0x4600;
/// 変えられない画面の情報を読む(`FBIOGET_FSCREENINFO`)。
const FBIOGET_FSCREENINFO: u32 = 0x4602;

/// `struct fb_bitfield`。
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct FbBitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

/// `struct fb_var_screeninfo`。
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct FbVarScreeninfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: FbBitfield,
    green: FbBitfield,
    blue: FbBitfield,
    transp: FbBitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

/// `struct fb_fix_screeninfo`(x86-64での並び)。
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct FbFixScreeninfo {
    id: [u8; 16],
    smem_start: u64,
    smem_len: u32,
    type_: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: u64,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

/// 開いたfbdevの装置と、対応づけた画面の領域。
pub struct Fbdev {
    file: File,
    map: NonNull<u8>,
    len: usize,
    info: ScreenInfo,
    layout: Layout,
}

impl Fbdev {
    /// 装置を開き、画面の情報を読んで、画面の領域を対応づける。
    pub fn open(path: &Path) -> Result<Self, FbError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|source| FbError::Open {
                path: path.display().to_string(),
                source,
            })?;

        let mut var = FbVarScreeninfo::default();
        let mut fix = FbFixScreeninfo::default();
        // SAFETY: 開いているfdに、Linuxのfbdevが決めた大きさの構造体を渡す。カーネルはその大きさ
        // だけを書き込む(大きさは下のテストで確かめている)。
        unsafe {
            query(
                &file,
                FBIOGET_VSCREENINFO,
                &mut var,
                "the variable screen information",
            )?;
            query(
                &file,
                FBIOGET_FSCREENINFO,
                &mut fix,
                "the fixed screen information",
            )?;
        }

        let info = ScreenInfo {
            width: var.xres,
            height: var.yres,
            x_offset: var.xoffset,
            y_offset: var.yoffset,
            bits_per_pixel: var.bits_per_pixel,
            line_length: fix.line_length,
            red: bitfield(var.red),
            green: bitfield(var.green),
            blue: bitfield(var.blue),
            transp: bitfield(var.transp),
        };
        let layout = Layout::new(&info)?;
        let len = fix.smem_len as usize;
        if len < layout.required_len() {
            return Err(FbError::ScreenTooSmall {
                len,
                needed: layout.required_len(),
            });
        }

        // SAFETY: 番地は任せ(null)、開いているfdの先頭から `len` バイトを共有で対応づける。
        // 失敗は MAP_FAILED で返るので、下で確かめる。
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if map == libc::MAP_FAILED {
            return Err(FbError::Map(io::Error::last_os_error()));
        }
        let map = NonNull::new(map.cast::<u8>())
            .ok_or_else(|| FbError::Map(io::ErrorKind::Other.into()))?;

        Ok(Fbdev {
            file,
            map,
            len,
            info,
            layout,
        })
    }

    /// 装置から読んだ画面の情報。
    pub fn info(&self) -> &ScreenInfo {
        &self.info
    }

    /// 確かめ終えた画面の形式。
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// 画面の領域の全体。
    pub fn pixels(&mut self) -> &mut [u8] {
        // SAFETY: `map` は `len` バイトの生きた対応づけで、このFbdevが捨てられるまで外さない。
        // `&mut self` を借りているので、このプログラムの中でほかに触るものは無い。
        unsafe { std::slice::from_raw_parts_mut(self.map.as_ptr(), self.len) }
    }

    /// 開いている装置。
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }

    /// 描画結果を画面の領域へ写し、`presenter` で画面へ反映する。
    pub fn show(
        &mut self,
        view: &FrameView<'_>,
        presenter: &mut (impl Presenter + ?Sized),
    ) -> Result<(), FbError> {
        let layout = self.layout;
        layout.blit(view, self.pixels())?;
        self.present(presenter)
    }

    /// 画面の領域へ書いたものを、`presenter` で画面へ反映する。
    pub fn present(&mut self, presenter: &mut (impl Presenter + ?Sized)) -> Result<(), FbError> {
        presenter
            .present(self.file.as_fd())
            .map_err(FbError::Present)
    }
}

impl Drop for Fbdev {
    fn drop(&mut self) {
        // SAFETY: `open` で対応づけた範囲を、そのまま外す。この後は誰も触らない。
        unsafe {
            libc::munmap(self.map.as_ptr().cast(), self.len);
        }
    }
}

fn bitfield(field: FbBitfield) -> Bitfield {
    Bitfield {
        offset: field.offset,
        length: field.length,
    }
}

/// 画面の情報を読むioctlを呼ぶ。
///
/// # Safety
///
/// `T` は、`request` に対してカーネルが書き込む構造体と同じ大きさ・同じ並びでなければならない。
unsafe fn query<T>(
    file: &File,
    request: u32,
    out: &mut T,
    what: &'static str,
) -> Result<(), FbError> {
    // ioctlの番号の型は、Cのライブラリによって違う(glibcはunsigned long、muslはint)。
    let result = unsafe { libc::ioctl(file.as_raw_fd(), request as _, out as *mut T) };
    if result == -1 {
        return Err(FbError::Query {
            what,
            source: io::Error::last_os_error(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    /// 構造体の大きさと要所の位置が、x86-64のLinuxの `linux/fb.h` と合っていること。
    #[test]
    fn the_structures_match_the_linux_abi() {
        assert_eq!(size_of::<FbBitfield>(), 12);
        assert_eq!(size_of::<FbVarScreeninfo>(), 160);
        assert_eq!(offset_of!(FbVarScreeninfo, bits_per_pixel), 24);
        assert_eq!(offset_of!(FbVarScreeninfo, red), 32);
        assert_eq!(offset_of!(FbVarScreeninfo, transp), 68);
        assert_eq!(offset_of!(FbVarScreeninfo, nonstd), 80);
        assert_eq!(size_of::<FbFixScreeninfo>(), 80);
        assert_eq!(offset_of!(FbFixScreeninfo, smem_len), 24);
        assert_eq!(offset_of!(FbFixScreeninfo, line_length), 48);
        assert_eq!(offset_of!(FbFixScreeninfo, mmio_start), 56);
    }

    #[test]
    fn a_missing_device_is_reported_by_name() {
        let result = Fbdev::open(Path::new("/nonexistent/fb0"));
        assert!(matches!(result, Err(FbError::Open { .. })));
    }

    /// fbdevでない装置は、画面の情報を読むところで断られる。
    #[test]
    fn a_device_that_is_not_a_framebuffer_is_rejected() {
        let result = Fbdev::open(Path::new("/dev/null"));
        assert!(matches!(result, Err(FbError::Query { .. })));
    }
}
