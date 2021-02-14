use std::collections::HashMap;

use chrono::{DateTime, Utc};
use consecrates::api::{CrateLinks, User, VersionLinks};

#[derive(Debug, Clone)]
pub(crate) struct Crate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub documentation: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub downloads: u64,
    pub recent_downloads: Option<u64>,
    pub categories: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    pub max_version: String,
    pub links: CrateLinks,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub exact_match: Option<bool>,

    pub readme: Option<String>,
}
