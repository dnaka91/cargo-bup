use std::{
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use serde::{de, Deserialize};
use url::Url;

/// Tracking information for the set of installed packages.
#[derive(Debug, Deserialize)]
pub struct CrateListingV2 {
    /// Map of every installed package.
    pub installs: BTreeMap<PackageId, InstallInfo>,
    /// Forwards compatibility. Unknown keys from future versions of Cargo
    /// will be stored here and retained when the file is saved.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

/// Identifier for a specific version of a package in a specific source.
#[derive(Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct PackageId {
    pub name: String,
    pub version: Version,
    pub source_id: SourceId,
}

impl<'de> de::Deserialize<'de> for PackageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        let mut s = string.splitn(3, ' ');

        let name = s.next().unwrap().to_owned();

        let version = s
            .next()
            .ok_or_else(|| de::Error::custom("invalid serialized PackageId"))?;
        let version = Version::parse(version.trim())
            .with_context(|| anyhow!("cannot parse `{version}` as a semver"))
            .map_err(de::Error::custom)?;

        let url = s
            .next()
            .ok_or_else(|| de::Error::custom("invalid serialized PackageId"))?;
        let url = url
            .strip_prefix('(')
            .and_then(|u| u.strip_suffix(')'))
            .ok_or_else(|| de::Error::custom("invalid serialized PackageId"))?;
        let source_id = SourceId::from_url(url).map_err(de::Error::custom)?;

        Ok(PackageId {
            name,
            version,
            source_id,
        })
    }
}

/// Unique identifier for a source of packages.
#[derive(Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct SourceId {
    /// The source URL.
    pub url: Url,
    /// The canonical version of the above url.
    pub canonical_url: CanonicalUrl,
    /// The source kind.
    pub kind: SourceKind,
    /// For example, the exact git revision of the specified branch for a Git Source.
    pub precise: Option<String>,
    /// Name of the registry source for alternate registries.
    /// WARNING: this is not always set for alt-registries when the name is not known.
    pub name: Option<String>,
}

impl SourceId {
    fn new(kind: SourceKind, url: Url, name: Option<&str>) -> Result<Self> {
        Ok(Self {
            kind,
            canonical_url: CanonicalUrl::new(&url)?,
            url,
            precise: None,
            name: name.map(ToOwned::to_owned),
        })
    }

    fn from_url(string: &str) -> Result<Self> {
        let mut parts = string.splitn(2, '+');
        let kind = parts.next().unwrap();
        let url = parts
            .next()
            .ok_or_else(|| anyhow!("invalid source `{string}`"))?;

        match kind {
            "git" => {
                let mut url = Url::parse(url).with_context(|| anyhow!("invalid url `{url}`"))?;
                let mut reference = GitReference::DefaultBranch;
                for (k, v) in url.query_pairs() {
                    match k.as_ref() {
                        "branch" | "ref" => reference = GitReference::Branch(v.into_owned()),
                        "rev" => reference = GitReference::Rev(v.into_owned()),
                        "tag" => reference = GitReference::Tag(v.into_owned()),
                        _ => {}
                    }
                }
                let precise = url.fragment().map(ToOwned::to_owned);
                url.set_fragment(None);
                url.set_query(None);
                Ok(SourceId::for_git(&url, reference)?.with_precise(precise))
            }
            "registry" => {
                let url = Url::parse(url).with_context(|| anyhow!("invalid url `{url}`"))?;
                Ok(SourceId::new(SourceKind::Registry, url, None)?
                    .with_precise(Some("locked".to_owned())))
            }
            "sparse" => {
                let url = Url::parse(string).with_context(|| anyhow!("invalid url `{url}`"))?;
                Ok(SourceId::new(SourceKind::Registry, url, None)?
                    .with_precise(Some("locked".to_owned())))
            }
            "path" => {
                let url = Url::parse(url).with_context(|| anyhow!("invalid url `{url}`"))?;
                SourceId::new(SourceKind::Path, url, None)
            }
            kind => bail!("unsupported source protocol: {kind}"),
        }
    }

    fn for_git(url: &Url, reference: GitReference) -> Result<SourceId> {
        SourceId::new(SourceKind::Git(reference), url.clone(), None)
    }

    fn with_precise(self, v: Option<String>) -> Self {
        Self { precise: v, ..self }
    }
}

/// The possible kinds of code source. Along with `SourceIdInner`, this fully defines the
/// source.
#[derive(Debug, PartialOrd, Ord, PartialEq, Eq)]
pub enum SourceKind {
    /// A git repository.
    Git(GitReference),
    /// A local path.
    Path,
    /// A remote registry.
    Registry,
}

/// Information to find a specific commit in a Git repository.
#[derive(Debug, PartialOrd, Ord, PartialEq, Eq)]
pub enum GitReference {
    /// From a tag.
    Tag(String),
    /// From a branch.
    Branch(String),
    /// From a specific revision.
    Rev(String),
    /// The default branch of the repository, the reference named `HEAD`.
    DefaultBranch,
}

#[derive(Debug, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct CanonicalUrl(pub Url);

impl CanonicalUrl {
    fn new(url: &Url) -> Result<Self> {
        let mut url = url.clone();

        // cannot-be-a-base-urls (e.g., `github.com:rust-lang/rustfmt.git`)
        // are not supported.
        if url.cannot_be_a_base() {
            bail!(
                "invalid url `{}`: cannot-be-a-base-URLs are not supported",
                url
            )
        }

        // Strip a trailing slash.
        if url.path().ends_with('/') {
            url.path_segments_mut().unwrap().pop_if_empty();
        }

        // For GitHub URLs specifically, just lower-case everything. GitHub
        // treats both the same, but they hash differently, and we're gonna be
        // hashing them. This wants a more general solution, and also we're
        // almost certainly not using the same case conversion rules that GitHub
        // does. (See issue #84)
        if url.host_str() == Some("github.com") {
            url = format!("https{}", &url[url::Position::AfterScheme..])
                .parse()
                .unwrap();
            let path = url.path().to_lowercase();
            url.set_path(&path);
        }

        // Repos can generally be accessed with or without `.git` extension.
        let needs_chopping = url.path().ends_with(".git");
        if needs_chopping {
            let last = {
                let last = url.path_segments().unwrap().next_back().unwrap();
                last[..last.len() - 4].to_owned()
            };
            url.path_segments_mut().unwrap().pop().push(&last);
        }

        // Ignore the protocol specifier (if any).
        if url.scheme().starts_with("sparse+") {
            // NOTE: it is illegal to use set_scheme to change sparse+http(s) to http(s).
            url = Url::parse(
                url.to_string()
                    .strip_prefix("sparse+")
                    .expect("we just found that prefix"),
            )
            .expect("a valid url without a protocol specifier should still be valid");
        }

        Ok(Self(url))
    }
}

/// Tracking information for the installation of a single package.
///
/// This track the settings that were used when the package was installed.
/// Future attempts to install the same package will check these settings to
/// determine if it needs to be rebuilt/reinstalled. If nothing has changed,
/// then Cargo will inform the user that it is "up to date".
///
/// This is only used for the v2 format.
#[derive(Debug, Deserialize)]
pub struct InstallInfo {
    pub version_req: Option<String>,
    /// Set of binary names installed.
    pub bins: BTreeSet<String>,
    /// Set of features explicitly enabled.
    pub features: BTreeSet<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    /// Either "debug" or "release".
    pub profile: String,
    /// The installation target.
    /// Either the host or the value specified in `--target`.
    /// None if unknown (when loading from v1).
    pub target: Option<String>,
    /// Output of `rustc -V`.
    /// None if unknown (when loading from v1).
    /// Currently not used, possibly may be used in the future.
    pub rustc: Option<String>,
    /// Forwards compatibility.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}
