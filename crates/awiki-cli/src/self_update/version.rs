use semver::Version;
use std::cmp::Ordering;

pub fn is_dev_version(raw: &str) -> bool {
    let version = raw.trim().to_ascii_lowercase();
    version.is_empty()
        || version == "dev"
        || version.contains("-dev")
        || version.starts_with("0.0.0-")
}

pub fn compare_versions(a: &str, b: &str) -> Option<i8> {
    let a = parse_version(a)?;
    let b = parse_version(b)?;
    // Total ordering includes build metadata; release precedence must ignore it.
    Some(match a.cmp_precedence(&b) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

fn parse_version(raw: &str) -> Option<Version> {
    let raw = raw.trim();
    let raw = raw.strip_prefix(['v', 'V']).unwrap_or(raw);
    // Keep the historical prefix and one/two-component version inputs. SemVer
    // owns validation and comparison after this small compatibility adapter.
    let suffix = raw.find(['-', '+']).unwrap_or(raw.len());
    let core = &raw[..suffix];
    let normalized = match core.split('.').count() {
        1 => format!("{core}.0.0{}", &raw[suffix..]),
        2 => format!("{core}.0{}", &raw[suffix..]),
        _ => raw.to_string(),
    };
    Version::parse(&normalized).ok()
}

#[cfg(test)]
#[path = "version_tests.rs"]
mod tests;
