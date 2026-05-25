pub fn is_dev_version(raw: &str) -> bool {
    let version = raw.trim().to_ascii_lowercase();
    version.is_empty()
        || version == "dev"
        || version.contains("-dev")
        || version.starts_with("0.0.0-")
}

pub fn compare_versions(a: &str, b: &str) -> Option<i8> {
    let a = SemVersion::parse(a)?;
    let b = SemVersion::parse(b)?;
    Some(a.compare(&b))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemVersion {
    major: u64,
    minor: u64,
    patch: u64,
    pre: String,
}

impl SemVersion {
    fn parse(raw: &str) -> Option<Self> {
        let mut raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        if raw.starts_with('v') || raw.starts_with('V') {
            raw = &raw[1..];
        }
        let (core, pre) = raw
            .split_once('-')
            .map(|(core, pre)| (core, pre))
            .unwrap_or((raw, ""));
        let parts: Vec<_> = core.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        Some(Self {
            major: parse_part(parts[0])?,
            minor: parts
                .get(1)
                .map(|part| parse_part(part))
                .unwrap_or(Some(0))?,
            patch: parts
                .get(2)
                .map(|part| parse_part(part))
                .unwrap_or(Some(0))?,
            pre: pre.to_string(),
        })
    }

    fn compare(&self, other: &Self) -> i8 {
        for ordering in [
            compare_u64(self.major, other.major),
            compare_u64(self.minor, other.minor),
            compare_u64(self.patch, other.patch),
        ] {
            if ordering != 0 {
                return ordering;
            }
        }
        compare_prerelease(&self.pre, &other.pre)
    }
}

fn parse_part(raw: &str) -> Option<u64> {
    if raw.is_empty() {
        return Some(0);
    }
    raw.parse::<u64>().ok()
}

fn compare_u64(a: u64, b: u64) -> i8 {
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Less => -1,
    }
}

fn compare_prerelease(a: &str, b: &str) -> i8 {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return 1;
    }
    if b.is_empty() {
        return -1;
    }
    let left: Vec<_> = a.split('.').collect();
    let right: Vec<_> = b.split('.').collect();
    let limit = left.len().max(right.len());
    for index in 0..limit {
        if index >= left.len() {
            return -1;
        }
        if index >= right.len() {
            return 1;
        }
        if left[index] == right[index] {
            continue;
        }
        let left_numeric = is_numeric_identifier(left[index]);
        let right_numeric = is_numeric_identifier(right[index]);
        match (left_numeric, right_numeric) {
            (true, true) => return compare_numeric_identifiers(left[index], right[index]),
            (true, false) => return -1,
            (false, true) => return 1,
            (false, false) if left[index] > right[index] => return 1,
            (false, false) => return -1,
        }
    }
    0
}

fn is_numeric_identifier(raw: &str) -> bool {
    !raw.is_empty() && raw.bytes().all(|byte| byte.is_ascii_digit())
}

fn compare_numeric_identifiers(a: &str, b: &str) -> i8 {
    let a = trim_numeric_identifier(a);
    let b = trim_numeric_identifier(b);
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal if a > b => 1,
        std::cmp::Ordering::Equal if a < b => -1,
        std::cmp::Ordering::Equal => 0,
    }
}

fn trim_numeric_identifier(raw: &str) -> &str {
    let trimmed = raw.trim_start_matches('0');
    if trimmed.is_empty() {
        "0"
    } else {
        trimmed
    }
}
