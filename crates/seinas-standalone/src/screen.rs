//! 絵を出す先。fbdevの装置か、メモリーの上の偽の画面。

use std::{
    error::Error,
    path::{Path, PathBuf},
};

use seinas_fbdev::{
    device::Fbdev,
    fake::FakeScreen,
    present::{Presenter, WriteThrough},
};
use seinas_render::FrameView;

pub enum Screen {
    /// fbdevの装置。`presenter` は、書いた絵を画面へ反映する処理(OSごとに差し替える)。
    Device {
        device: Fbdev,
        presenter: Box<dyn Presenter>,
        path: PathBuf,
    },
    /// 偽の画面。`dump` があれば、描くたびにPPM形式で書き出す。
    Fake {
        screen: FakeScreen,
        dump: Option<PathBuf>,
    },
}

impl Screen {
    /// 装置を開く。
    pub fn open(path: &Path) -> Result<Self, Box<dyn Error>> {
        Ok(Screen::Device {
            device: Fbdev::open(path)?,
            // Linuxのfbdevは、書けばそのまま映る。
            presenter: Box::new(WriteThrough),
            path: path.to_owned(),
        })
    }

    /// 偽の画面を作る。
    pub fn fake(width: u32, height: u32, dump: Option<PathBuf>) -> Result<Self, Box<dyn Error>> {
        Ok(Screen::Fake {
            screen: FakeScreen::new(width, height)?,
            dump,
        })
    }

    /// 画面の幅と高さ(画素)。
    pub fn size(&self) -> (i32, i32) {
        let layout = match self {
            Screen::Device { device, .. } => device.layout(),
            Screen::Fake { screen, .. } => screen.layout(),
        };
        (layout.width() as i32, layout.height() as i32)
    }

    /// ログに出すための、画面の説明。
    pub fn describe(&self) -> String {
        match self {
            Screen::Device { path, .. } => path.display().to_string(),
            Screen::Fake {
                dump: Some(path), ..
            } => format!("fake, dumped to {}", path.display()),
            Screen::Fake { dump: None, .. } => "fake".to_owned(),
        }
    }

    /// 描画結果を画面へ出す。
    pub fn show(&mut self, view: &FrameView<'_>) -> Result<(), Box<dyn Error>> {
        match self {
            Screen::Device {
                device, presenter, ..
            } => device.show(view, presenter.as_mut())?,
            Screen::Fake { screen, dump } => {
                screen.show(view)?;
                if let Some(path) = dump {
                    screen
                        .write_ppm(path)
                        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                }
            }
        }
        Ok(())
    }
}
