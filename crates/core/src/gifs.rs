//! Port of packages/core/src/gifs.ts: the Klipy GIF API. The key rides in the
//! URL path. Klipy's attribution rules require the search placeholder "Search KLIPY".

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
pub fn merge_gif_favorites(
    current: &HashMap<String, GifFavorite>,
    incoming: &HashMap<String, GifFavorite>,
) -> HashMap<String, GifFavorite> {
    let _ = (current, incoming);
    unimplemented!()
}

/// Live favorites, newest first.
pub fn favorite_gifs(favorites: &HashMap<String, GifFavorite>) -> Vec<Gif> {
    let _ = favorites;
    unimplemented!()
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
        let _ = (page, &self.api_key, &self.http);
        unimplemented!()
    }

    pub async fn search(&self, query: &str, page: u32) -> anyhow::Result<GifPage> {
        let _ = (query, page);
        unimplemented!()
    }
}

/// Saves the full GIF as `<attachments_dir>/klipy-<id>.gif`, ready to send. Reuses an existing file.
pub async fn download_gif(http: &reqwest::Client, gif: &Gif, attachments_dir: &Path) -> anyhow::Result<PathBuf> {
    let _ = (http, gif, attachments_dir);
    unimplemented!()
}

/// Saves the preview as `<attachments_dir>/klipy-<id>-preview.gif`.
pub async fn download_gif_preview(http: &reqwest::Client, gif: &Gif, attachments_dir: &Path) -> anyhow::Result<PathBuf> {
    let _ = (http, gif, attachments_dir);
    unimplemented!()
}
