//! Port of packages/core/src/findmy.ts.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Millis;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendLocation {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub addresses: Vec<String>,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub accuracy: Option<f64>,
    pub timestamp: Millis,
    #[serde(default)]
    pub label: Option<String>,
    pub is_sharing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLocation {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub accuracy: Option<f64>,
    /// 0 to 1 charge level, when Apple reports one.
    #[serde(default)]
    pub battery: Option<f64>,
    #[serde(default)]
    pub timestamp: Option<Millis>,
}

/// Emails compare case-insensitively; phone numbers on their last 9 digits.
pub fn normalize_address(address: &str) -> String {
    let _ = address;
    unimplemented!()
}

/// Every address of the contact that owns `address`.
pub fn contact_addresses(contacts: &[crate::model::Contact], address: &str) -> Vec<String> {
    let _ = (contacts, address);
    unimplemented!()
}

pub fn match_friend<'a>(friends: &'a [FriendLocation], addresses: &[String]) -> Option<&'a FriendLocation> {
    let _ = (friends, addresses);
    unimplemented!()
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapTile {
    pub path: PathBuf,
    /// The point's pixel offset inside the 256 px tile.
    pub px: u32,
    pub py: u32,
}

/// Downloads or reuses an OpenStreetMap tile under `<cache_dir>/tiles`, refetched after a day.
/// User-Agent `messages-linux/0.1 (github.com/FormalSnake/messages)`.
pub async fn tile_for(http: &reqwest::Client, lat: f64, lon: f64, zoom: u8, cache_dir: &Path) -> anyhow::Result<MapTile> {
    let _ = (http, lat, lon, zoom, cache_dir);
    unimplemented!()
}
