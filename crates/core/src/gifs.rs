//! Port of packages/core/src/gifs.ts: the Klipy GIF API. The key rides in the
//! URL path. Klipy's attribution rules require the search placeholder "Search KLIPY".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::Millis;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gif {
    pub id: String,
    pub preview_url: String,
    pub gif_url: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifPage {
    pub items: Vec<Gif>,
    pub has_more: bool,
}

/// An unfavorite keeps the entry as a `removed` tombstone so it wins over a stale favorite from another client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GifFavorite {
    pub gif: Gif,
    pub updated_at: Millis,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
}

/// Per id, the newer `updated_at` wins; a tie keeps `current`.
pub fn merge_gif_favorites(current: &HashMap<String, GifFavorite>, incoming: &HashMap<String, GifFavorite>) -> HashMap<String, GifFavorite> {
    let mut merged = current.clone();
    for (id, entry) in incoming {
        let replace = match merged.get(id) {
            Some(existing) => entry.updated_at > existing.updated_at,
            None => true,
        };
        if replace {
            merged.insert(id.clone(), entry.clone());
        }
    }
    merged
}

/// Live favorites, newest first.
pub fn favorite_gifs(favorites: &HashMap<String, GifFavorite>) -> Vec<Gif> {
    let mut entries: Vec<&GifFavorite> = favorites.values().filter(|entry| !entry.removed).collect();
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    entries.into_iter().map(|entry| entry.gif.clone()).collect()
}

#[derive(Deserialize)]
struct KlipyFile {
    url: String,
    #[serde(default, deserialize_with = "dimension")]
    width: u32,
    #[serde(default, deserialize_with = "dimension")]
    height: u32,
}

/// Any JSON number, or nothing; one odd size must not cost the whole page.
fn dimension<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| value.as_f64()).filter(|number| number.is_finite() && *number >= 0.0).map_or(0, |number| number.round() as u32))
}

/// `String(item.id)`: Klipy sends a number, but a string id is taken as it is.
fn item_id<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(id) => id,
        other => other.to_string(),
    })
}

/// Klipy serves every item at four sizes; each size may carry gif/webp/mp4 variants.
#[derive(Deserialize, Default)]
struct KlipySize {
    gif: Option<KlipyFile>,
}

#[derive(Deserialize, Default)]
struct KlipyFileSizes {
    hd: Option<KlipySize>,
    md: Option<KlipySize>,
    sm: Option<KlipySize>,
    xs: Option<KlipySize>,
}

#[derive(Deserialize)]
struct KlipyItem {
    #[serde(deserialize_with = "item_id")]
    id: String,
    #[serde(default)]
    file: KlipyFileSizes,
}

#[derive(Deserialize)]
struct KlipyData {
    data: Vec<KlipyItem>,
    has_next: bool,
}

#[derive(Deserialize)]
struct KlipyResponse {
    result: bool,
    data: KlipyData,
}

fn to_gif(item: &KlipyItem) -> Option<Gif> {
    let full = item
        .file
        .hd
        .as_ref()
        .and_then(|s| s.gif.as_ref())
        .or_else(|| item.file.md.as_ref().and_then(|s| s.gif.as_ref()))
        .or_else(|| item.file.sm.as_ref().and_then(|s| s.gif.as_ref()))
        .or_else(|| item.file.xs.as_ref().and_then(|s| s.gif.as_ref()))?;
    let preview = item.file.sm.as_ref().and_then(|s| s.gif.as_ref()).or_else(|| item.file.md.as_ref().and_then(|s| s.gif.as_ref())).unwrap_or(full);
    Some(Gif { id: item.id.clone(), preview_url: preview.url.clone(), gif_url: full.url.clone(), width: full.width, height: full.height })
}

const BASE_URL: &str = "https://api.klipy.com/api/v1";
const TIMEOUT: Duration = Duration::from_secs(10);

fn build_url(api_key: &str, endpoint: &str, params: &[(&str, String)]) -> url::Url {
    let mut url = url::Url::parse(&format!("{BASE_URL}/{api_key}/gifs/{endpoint}")).expect("klipy base url is always valid");
    for (key, value) in params {
        url.query_pairs_mut().append_pair(key, value);
    }
    url
}

fn parse_klipy_response(endpoint: &str, status: u16, body: &[u8]) -> anyhow::Result<GifPage> {
    if !(200..300).contains(&status) {
        anyhow::bail!("klipy: {endpoint} returned {status}");
    }
    let parsed: KlipyResponse = serde_json::from_slice(body)?;
    if !parsed.result {
        anyhow::bail!("klipy: {endpoint} request failed");
    }
    let items = parsed.data.data.iter().filter_map(to_gif).collect();
    Ok(GifPage { items, has_more: parsed.data.has_next })
}

/// `https://api.klipy.com/api/v1/<key>/gifs/{trending,search}`, 10 s timeout.
pub struct KlipyClient {
    api_key: String,
    http: reqwest::Client,
}

impl KlipyClient {
    pub fn new(api_key: String, http: reqwest::Client) -> Self {
        Self { api_key, http }
    }

    pub async fn trending(&self, page: u32) -> anyhow::Result<GifPage> {
        self.fetch_page("trending", &[("page", page.to_string())]).await
    }

    pub async fn search(&self, query: &str, page: u32) -> anyhow::Result<GifPage> {
        self.fetch_page("search", &[("page", page.to_string()), ("q", query.to_owned())]).await
    }

    async fn fetch_page(&self, endpoint: &str, params: &[(&str, String)]) -> anyhow::Result<GifPage> {
        let url = build_url(&self.api_key, endpoint, params);
        let response = self.http.get(url).timeout(TIMEOUT).send().await?;
        let status = response.status().as_u16();
        let body = response.bytes().await?;
        parse_klipy_response(endpoint, status, &body)
    }
}

async fn download_to(http: &reqwest::Client, url: &str, file_path: &Path) -> anyhow::Result<PathBuf> {
    if tokio::fs::try_exists(file_path).await.unwrap_or(false) {
        return Ok(file_path.to_path_buf());
    }
    let response = http.get(url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("klipy: download returned {}", response.status().as_u16());
    }
    let bytes = response.bytes().await?;
    if let Some(dir) = file_path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    // Written aside and renamed, so a download cut short is never taken for the file next time.
    let mut part = file_path.as_os_str().to_owned();
    part.push(".part");
    tokio::fs::write(&part, &bytes).await?;
    tokio::fs::rename(&part, file_path).await?;
    Ok(file_path.to_path_buf())
}

/// Saves the full GIF as `<attachments_dir>/klipy-<id>.gif`, ready to send. Reuses an existing file.
pub async fn download_gif(http: &reqwest::Client, gif: &Gif, attachments_dir: &Path) -> anyhow::Result<PathBuf> {
    download_to(http, &gif.gif_url, &attachments_dir.join(format!("klipy-{}.gif", gif.id))).await
}

/// Saves the preview as `<attachments_dir>/klipy-<id>-preview.gif`.
pub async fn download_gif_preview(http: &reqwest::Client, gif: &Gif, attachments_dir: &Path) -> anyhow::Result<PathBuf> {
    download_to(http, &gif.preview_url, &attachments_dir.join(format!("klipy-{}-preview.gif", gif.id))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_gif(id: &str) -> Gif {
        Gif { id: id.to_owned(), preview_url: format!("https://static.klipy.com/{id}/sm.gif"), gif_url: format!("https://static.klipy.com/{id}/hd.gif"), width: 200, height: 200 }
    }

    fn klipy_item_json(id: u64, hd: Option<&str>, sm: Option<&str>) -> serde_json::Value {
        let file = |url: &str| serde_json::json!({ "gif": { "url": url, "width": 200, "height": 200, "size": 1024 } });
        serde_json::json!({
            "id": id,
            "slug": format!("slug-{id}"),
            "title": format!("Gif {id}"),
            "file": {
                "hd": file(hd.unwrap_or(&format!("https://static.klipy.com/{id}/hd.gif"))),
                "md": file(&format!("https://static.klipy.com/{id}/md.gif")),
                "sm": file(sm.unwrap_or(&format!("https://static.klipy.com/{id}/sm.gif"))),
                "xs": file(&format!("https://static.klipy.com/{id}/xs.gif")),
            }
        })
    }

    fn klipy_response_json(items: Vec<serde_json::Value>, has_next: bool) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "result": true, "data": { "data": items, "current_page": 1, "per_page": 24, "has_next": has_next } })).unwrap()
    }

    #[test]
    fn builds_the_trending_url_with_the_app_key_in_the_path() {
        let url = build_url("app-key", "trending", &[("page", "2".to_owned())]);
        assert_eq!(url.path(), "/api/v1/app-key/gifs/trending");
        assert_eq!(url.query_pairs().find(|(k, _)| k == "page").map(|(_, v)| v.into_owned()), Some("2".to_owned()));
    }

    #[test]
    fn builds_the_search_url_with_the_query_string() {
        let url = build_url("app-key", "search", &[("page", "1".to_owned()), ("q", "cats".to_owned())]);
        assert_eq!(url.path(), "/api/v1/app-key/gifs/search");
        assert_eq!(url.query_pairs().find(|(k, _)| k == "q").map(|(_, v)| v.into_owned()), Some("cats".to_owned()));
    }

    #[test]
    fn parses_a_trending_page() {
        let body = klipy_response_json(vec![klipy_item_json(1, None, None)], false);
        let page = parse_klipy_response("trending", 200, &body).unwrap();
        assert!(!page.has_more);
        assert_eq!(page.items, vec![Gif { id: "1".to_owned(), preview_url: "https://static.klipy.com/1/sm.gif".to_owned(), gif_url: "https://static.klipy.com/1/hd.gif".to_owned(), width: 200, height: 200 }]);
    }

    #[test]
    fn reports_has_more_from_a_search_page() {
        let body = klipy_response_json(vec![klipy_item_json(1, None, None), klipy_item_json(2, None, None)], true);
        let page = parse_klipy_response("search", 200, &body).unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.has_more);
    }

    #[test]
    fn skips_an_item_with_no_gif_file_instead_of_failing_the_whole_page() {
        let broken = serde_json::json!({ "id": 3, "slug": "broken", "title": "Broken", "file": {} });
        let body = klipy_response_json(vec![klipy_item_json(1, None, None), broken], false);
        let page = parse_klipy_response("trending", 200, &body).unwrap();
        assert_eq!(page.items.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["1"]);
    }

    #[test]
    fn errors_when_the_server_answers_with_an_error_status() {
        let error = parse_klipy_response("trending", 429, b"{}").unwrap_err();
        assert!(error.to_string().contains("429"));
    }

    #[test]
    fn errors_when_result_is_false() {
        let body = serde_json::to_vec(&serde_json::json!({ "result": false, "data": { "data": [], "current_page": 1, "per_page": 24, "has_next": false } })).unwrap();
        let error = parse_klipy_response("trending", 200, &body).unwrap_err();
        assert!(error.to_string().contains("klipy"));
    }

    #[tokio::test]
    async fn keeps_the_file_already_on_disk_instead_of_downloading_again() {
        let dir = tempdir();
        let gif = test_gif("42");
        let target = dir.join("klipy-42.gif");
        tokio::fs::write(&target, "already here").await.unwrap();
        let http = reqwest::Client::new();
        let saved = download_gif(&http, &gif, &dir).await.unwrap();
        assert_eq!(saved, target);
        assert_eq!(tokio::fs::read_to_string(&saved).await.unwrap(), "already here");
    }

    #[tokio::test]
    async fn downloads_the_full_gif_and_the_preview_separately_into_the_cache() {
        let server = crate::testing::serve(|request| crate::testing::Response::bytes(200, request.path().as_bytes())).await;
        let dir = tempdir().join("attachments");
        let gif = Gif { id: "7".into(), preview_url: format!("{}/7/sm.gif", server.url), gif_url: format!("{}/7/hd.gif", server.url), width: 1, height: 1 };
        let http = reqwest::Client::new();
        let full = download_gif(&http, &gif, &dir).await.unwrap();
        let preview = download_gif_preview(&http, &gif, &dir).await.unwrap();
        assert_eq!(full, dir.join("klipy-7.gif"));
        assert_eq!(preview, dir.join("klipy-7-preview.gif"));
        assert_eq!(std::fs::read(&full).unwrap(), b"/7/hd.gif");
        assert_eq!(std::fs::read(&preview).unwrap(), b"/7/sm.gif");
        assert!(!dir.join("klipy-7.gif.part").exists());
    }

    #[tokio::test]
    async fn errors_when_the_download_response_is_not_ok_and_leaves_nothing_behind() {
        let server = crate::testing::serve(|_| crate::testing::Response::bytes(404, b"")).await;
        let dir = tempdir();
        let gif = Gif { id: "8".into(), preview_url: String::new(), gif_url: format!("{}/8.gif", server.url), width: 1, height: 1 };
        let error = download_gif(&reqwest::Client::new(), &gif, &dir).await.unwrap_err();
        assert!(error.to_string().contains("404"));
        assert!(!dir.join("klipy-8.gif").exists());
    }

    #[test]
    fn takes_a_string_id_and_a_fractional_size() {
        let item = serde_json::json!({ "id": "abc", "file": { "hd": { "gif": { "url": "u", "width": 200.4, "height": null } } } });
        let body = klipy_response_json(vec![item], false);
        let page = parse_klipy_response("trending", 200, &body).unwrap();
        assert_eq!(page.items[0].id, "abc");
        assert_eq!((page.items[0].width, page.items[0].height), (200, 0));
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("messages-gifs-test-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lets_a_newer_updated_at_win() {
        let current: HashMap<String, GifFavorite> = [("a".to_owned(), GifFavorite { gif: test_gif("a"), updated_at: 100, removed: false })].into_iter().collect();
        let incoming: HashMap<String, GifFavorite> = [("a".to_owned(), GifFavorite { gif: test_gif("a"), updated_at: 200, removed: false })].into_iter().collect();
        let merged = merge_gif_favorites(&current, &incoming);
        assert_eq!(merged["a"], GifFavorite { gif: test_gif("a"), updated_at: 200, removed: false });
    }

    #[test]
    fn keeps_the_current_entry_on_an_equal_updated_at() {
        let current: HashMap<String, GifFavorite> = [("a".to_owned(), GifFavorite { gif: test_gif("a"), updated_at: 100, removed: false })].into_iter().collect();
        let incoming: HashMap<String, GifFavorite> = [("a".to_owned(), GifFavorite { gif: test_gif("a"), updated_at: 100, removed: true })].into_iter().collect();
        let merged = merge_gif_favorites(&current, &incoming);
        assert_eq!(merged["a"], GifFavorite { gif: test_gif("a"), updated_at: 100, removed: false });
    }

    #[test]
    fn lets_a_tombstone_beat_an_older_favorite() {
        let current: HashMap<String, GifFavorite> = [("a".to_owned(), GifFavorite { gif: test_gif("a"), updated_at: 100, removed: false })].into_iter().collect();
        let incoming: HashMap<String, GifFavorite> = [("a".to_owned(), GifFavorite { gif: test_gif("a"), updated_at: 200, removed: true })].into_iter().collect();
        let merged = merge_gif_favorites(&current, &incoming);
        assert_eq!(merged["a"], GifFavorite { gif: test_gif("a"), updated_at: 200, removed: true });
    }

    #[test]
    fn merges_an_id_absent_from_current_in_from_incoming() {
        let incoming: HashMap<String, GifFavorite> = [("b".to_owned(), GifFavorite { gif: test_gif("b"), updated_at: 5, removed: false })].into_iter().collect();
        let merged = merge_gif_favorites(&HashMap::new(), &incoming);
        assert_eq!(merged["b"], GifFavorite { gif: test_gif("b"), updated_at: 5, removed: false });
    }

    #[test]
    fn returns_the_live_favorites_newest_first() {
        let favorites: HashMap<String, GifFavorite> = [
            ("a".to_owned(), GifFavorite { gif: test_gif("a"), updated_at: 100, removed: false }),
            ("b".to_owned(), GifFavorite { gif: test_gif("b"), updated_at: 300, removed: false }),
            ("c".to_owned(), GifFavorite { gif: test_gif("c"), updated_at: 200, removed: false }),
        ]
        .into_iter()
        .collect();
        assert_eq!(favorite_gifs(&favorites).iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["b", "c", "a"]);
    }

    #[test]
    fn excludes_a_removed_tombstone() {
        let favorites: HashMap<String, GifFavorite> =
            [("a".to_owned(), GifFavorite { gif: test_gif("a"), updated_at: 100, removed: false }), ("b".to_owned(), GifFavorite { gif: test_gif("b"), updated_at: 200, removed: true })]
                .into_iter()
                .collect();
        assert_eq!(favorite_gifs(&favorites).iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
    }
}
