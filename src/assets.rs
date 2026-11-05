//! Assets embedded in the binary: Stahl's own, over the component library's.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString, assets::Assets as ComponentAssets};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets/ui"]
#[exclude = "*/LICENSE-*"]
struct OwnAssets;

#[derive(Clone, Copy)]
pub struct StahlAssets;

pub const STAHL_MARK: &str = "stahl-mark.svg";

impl AssetSource for StahlAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match OwnAssets::get(path) {
            Some(file) => Ok(Some(file.data)),
            None => ComponentAssets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = ComponentAssets.list(path)?;
        assets.extend(
            OwnAssets::iter()
                .filter(|name| name.starts_with(path))
                .map(|name| SharedString::from(name.into_owned())),
        );
        Ok(assets)
    }
}
