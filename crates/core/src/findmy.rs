use serde::{Deserialize, Serialize};

use crate::model::Millis;

/// Epoch milliseconds from any JSON number. The agent computes some of them
/// from Apple-epoch seconds with a fraction, so they can arrive as `1.7e12`-style floats.
pub(crate) fn millis<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Millis, D::Error> {
    Ok(opt_millis(deserializer)?.unwrap_or(0))
}

pub(crate) fn opt_millis<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Millis>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| value.as_i64().or_else(|| value.as_f64().filter(|number| number.is_finite()).map(|number| number as Millis))))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendLocation {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub addresses: Vec<String>,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub accuracy: Option<f64>,
    #[serde(default, deserialize_with = "millis")]
    pub timestamp: Millis,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "sharing")]
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
    #[serde(default, deserialize_with = "opt_millis")]
    pub timestamp: Option<Millis>,
}

/// Every row the agent serves is an active share; a missing flag reads as one.
fn sharing() -> bool {
    true
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

/// A map image of one coordinate, rendered by the Mac agent with MapKit.
#[derive(Clone, Debug, PartialEq)]
pub struct MapSnapshotRequest {
    pub latitude: f64,
    pub longitude: f64,
    /// Points; the image is `width * scale` by `height * scale` pixels.
    pub width: u32,
    pub height: u32,
    pub scale: u32,
    pub dark: bool,
    /// Metres shown across the wider side.
    pub span: u32,
}

impl MapSnapshotRequest {
    /// Five decimals is about a metre, and it is the precision the agent renders and caches at.
    pub fn rounded(&self) -> (f64, f64) {
        let round = |value: f64| (value * 1e5).round() / 1e5;
        (round(self.latitude), round(self.longitude))
    }

    pub fn query(&self) -> String {
        let (lat, lon) = self.rounded();
        format!("lat={lat:.5}&lon={lon:.5}&w={}&h={}&scale={}&dark={}&span={}", self.width, self.height, self.scale, u8::from(self.dark), self.span)
    }

    pub fn file_name(&self) -> String {
        let (lat, lon) = self.rounded();
        format!("{lat:.5}_{lon:.5}_{}x{}@{}_{}_{}.png", self.width, self.height, self.scale, if self.dark { "dark" } else { "light" }, self.span)
    }
}

/// Apple Maps on a Mac, Google Maps everywhere else.
pub fn maps_url(latitude: f64, longitude: f64, name: Option<&str>) -> String {
    maps_url_for(cfg!(target_os = "macos"), latitude, longitude, name)
}

fn maps_url_for(apple: bool, latitude: f64, longitude: f64, name: Option<&str>) -> String {
    if apple {
        let mut url = format!("https://maps.apple.com/?ll={latitude},{longitude}");
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            url.push_str("&q=");
            url.push_str(&url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>());
        }
        url
    } else {
        format!("https://www.google.com/maps/search/?api=1&query={latitude},{longitude}")
    }
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
    fn reads_fractional_and_missing_timestamps_instead_of_dropping_the_whole_list() {
        let friends: Vec<FriendLocation> = serde_json::from_value(serde_json::json!([
            { "id": "a", "addresses": ["+34600111222"], "latitude": 28.1, "longitude": -15.4, "timestamp": 1_720_471_200_123.456_f64, "isSharing": true },
            { "id": "b", "latitude": 28.1, "longitude": -15.4 },
        ]))
        .unwrap();
        assert_eq!(friends[0].timestamp, 1_720_471_200_123);
        assert_eq!(friends[1].timestamp, 0);
        assert!(friends[1].is_sharing);
        let device: DeviceLocation = serde_json::from_value(serde_json::json!({ "id": "d", "name": "Mac", "latitude": 1.0, "longitude": 2.0, "timestamp": 5.5 })).unwrap();
        assert_eq!(device.timestamp, Some(5));
    }

    #[test]
    fn snapshot_requests_round_to_a_metre_for_the_query_and_the_file_name() {
        let request = MapSnapshotRequest { latitude: 28.123456789, longitude: -15.436349, width: 248, height: 160, scale: 2, dark: true, span: 1500 };
        assert_eq!(request.query(), "lat=28.12346&lon=-15.43635&w=248&h=160&scale=2&dark=1&span=1500");
        let nearby = MapSnapshotRequest { latitude: 28.1234601, ..request.clone() };
        assert_eq!(request.file_name(), nearby.file_name());
        assert_ne!(request.file_name(), MapSnapshotRequest { dark: false, ..request }.file_name());
    }

    #[test]
    fn maps_url_names_the_place_for_apple_and_only_the_coordinate_for_google() {
        assert_eq!(maps_url_for(true, 37.7955, -122.3937, Some("Alex Rivera")), "https://maps.apple.com/?ll=37.7955,-122.3937&q=Alex+Rivera");
        assert_eq!(maps_url_for(true, 1.5, 2.5, None), "https://maps.apple.com/?ll=1.5,2.5");
        assert_eq!(maps_url_for(false, 37.7955, -122.3937, Some("Alex")), "https://www.google.com/maps/search/?api=1&query=37.7955,-122.3937");
    }
}
