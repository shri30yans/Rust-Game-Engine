//! Test access to the same mounted pack stream used by engine-owned headless startup.
use app::GameEntry;
use engine_assets::{ContentPack, PackReader};

use crate::ImagineEntry;

pub fn pack() -> Result<ContentPack, Box<dyn std::error::Error>> {
    let stream =
        platform::ContentSource::headless().open_content(ImagineEntry::CONTENT.as_str())?;
    Ok(PackReader::read(stream)?)
}
