//! Port of packages/core/src/findmy.ts.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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
    if address.contains('@') {
        return address.to_lowercase();
    }
    let digits: String = address.chars().filter(char::is_ascii_digit).collect();
    if digits.len() >= 9 {
        digits[digits.len() - 9..].to_owned()
    } else {
        digits
    }
}

/// Every address of the contact that owns `address`.
pub fn contact_addresses(contacts: &[crate::model::Contact], address: &str) -> Vec<String> {
    let wanted = normalize_address(address);
    contacts.iter().find(|contact| contact.addresses.iter().any(|item| normalize_address(item) == wanted)).map(|contact| contact.addresses.clone()).unwrap_or_default()
}

pub fn match_friend<'a>(friends: &'a [FriendLocation], addresses: &[String]) -> Option<&'a FriendLocation> {
    let wanted: std::collections::HashSet<String> = addresses.iter().map(|a| normalize_address(a)).collect();
    friends.iter().find(|friend| friend.addresses.iter().any(|address| wanted.contains(&normalize_address(address))))
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapTile {
    pub path: PathBuf,
    /// The point's pixel offset inside the 256 px tile.
    pub px: u32,
    pub py: u32,
}

const TILE_SIZE: f64 = 256.0;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

struct TileCoord {
    x: u32,
    y: u32,
    px: u32,
    py: u32,
}

fn tile_coord(lat: f64, lon: f64, zoom: u8) -> TileCoord {
    let scale = 2f64.powi(zoom as i32);
    let x_fraction = ((lon + 180.0) / 360.0) * scale;
    let lat_rad = lat.to_radians();
    let y_fraction = ((1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0) * scale;
    let x = x_fraction.floor();
    let y = y_fraction.floor();
    TileCoord { x: x as u32, y: y as u32, px: ((x_fraction - x) * TILE_SIZE) as u32, py: ((y_fraction - y) * TILE_SIZE) as u32 }
}

/// Downloads or reuses an OpenStreetMap tile under `<cache_dir>/tiles`, refetched after a day.
/// User-Agent `messages-linux/0.1 (github.com/FormalSnake/messages)`.
pub async fn tile_for(http: &reqwest::Client, lat: f64, lon: f64, zoom: u8, cache_dir: &Path) -> anyhow::Result<MapTile> {
    let coord = tile_coord(lat, lon, zoom);
    let tiles_dir = cache_dir.join("tiles");
    tokio::fs::create_dir_all(&tiles_dir).await?;
    let tile_path = tiles_dir.join(format!("{zoom}-{}-{}.png", coord.x, coord.y));

    let fresh = tokio::fs::metadata(&tile_path)
        .await
        .ok()
        .and_then(|meta| meta.modified().ok())
        .is_some_and(|modified| SystemTime::now().duration_since(modified).map(|age| age < DAY).unwrap_or(false));

    if !fresh {
        let url = format!("https://tile.openstreetmap.org/{zoom}/{}/{}.png", coord.x, coord.y);
        let response = http.get(&url).header(reqwest::header::USER_AGENT, "messages-linux/0.1 (github.com/FormalSnake/messages)").send().await?;
        if !response.status().is_success() {
            anyhow::bail!("findmy: tile fetch for {zoom}/{}/{} returned {}", coord.x, coord.y, response.status().as_u16());
        }
        let bytes = response.bytes().await?;
        tokio::fs::write(&tile_path, &bytes).await?;
    }

    Ok(MapTile { path: tile_path, px: coord.px, py: coord.py })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Contact;

    #[test]
    fn emails_compare_case_insensitively() {
        assert_eq!(normalize_address("Alex@Example.com"), "alex@example.com");
    }

    #[test]
    fn phone_numbers_compare_on_their_last_nine_digits() {
        assert_eq!(normalize_address("+34 600 111 222"), normalize_address("600111222"));
    }

    #[test]
    fn contact_addresses_finds_every_address_of_the_owning_contact() {
        let contacts = vec![Contact { id: "1".to_owned(), name: "Alex".to_owned(), addresses: vec!["+34600111222".to_owned(), "alex@example.com".to_owned()], avatar: None }];
        assert_eq!(contact_addresses(&contacts, "alex@example.com"), vec!["+34600111222", "alex@example.com"]);
        assert!(contact_addresses(&contacts, "nobody@example.com").is_empty());
    }

    #[test]
    fn match_friend_matches_on_any_normalized_address() {
        let friends = vec![FriendLocation {
            id: "f1".to_owned(),
            name: None,
            addresses: vec!["+34600111222".to_owned()],
            latitude: 28.1,
            longitude: -15.4,
            accuracy: None,
            timestamp: 1,
            label: None,
            is_sharing: true,
        }];
        assert!(match_friend(&friends, &["600111222".to_owned()]).is_some());
        assert!(match_friend(&friends, &["nomatch".to_owned()]).is_none());
    }

    #[test]
    fn tile_coord_is_stable_for_a_known_point() {
        // Las Palmas, roughly. Zoom 15 tiles are well documented for sanity checking.
        let coord = tile_coord(28.1235, -15.4363, 15);
        assert!(coord.px < 256 && coord.py < 256);
    }
}
