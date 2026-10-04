//! 書いた絵を画面へ反映する処理。
//!
//! 画面の領域へ書いた後に何が要るかは、OSによって違う。その違いをここに閉じ込め、
//! ほかの部分は [`Presenter`] を呼ぶだけにする。

use std::{io, os::fd::BorrowedFd};

/// 画面の領域へ書いた絵を、画面へ反映するもの。
pub trait Presenter {
    /// 1枚ぶんを書き終えた後に呼ばれる。`device` は開いている装置。
    fn present(&mut self, device: BorrowedFd<'_>) -> io::Result<()>;
}

/// Linuxのfbdev向け。書けばそのまま映るので、何もしない。
#[derive(Debug, Default, Clone, Copy)]
pub struct WriteThrough;

impl Presenter for WriteThrough {
    fn present(&mut self, _device: BorrowedFd<'_>) -> io::Result<()> {
        Ok(())
    }
}
