pub fn commit() -> &'static str {
    env!("NEWTYPE_COMMIT")
}

pub fn full_version(version: &str, commit: &str) -> String {
    format!("{version}+{commit}")
}

#[cfg(test)]
mod tests {
    use super::full_version;
    use semver::Version;

    fn v(value: &str) -> Version {
        Version::parse(value).unwrap()
    }

    #[test]
    fn dev_versions_increase_under_the_updater_ordering() {
        // tauri-plugin-updater accepts a release when `release.version > current`,
        // and that `>` is semver::Version's Ord.
        let versions = [
            "0.1.0-dev.1",
            "0.1.0-dev.2",
            "0.1.0-dev.10",
            "0.1.0",
            "0.2.0-dev.1",
            "0.2.0",
        ];
        for pair in versions.windows(2) {
            assert!(
                v(pair[0]) < v(pair[1]),
                "expected {} < {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn build_metadata_does_not_order_by_time() {
        // Same prerelease, different `+sha`. Ord compares the sha alphabetically,
        // so a newer build can sort as older. Keep the sha out of the updater version.
        let later_sha = v("0.1.0-dev.2+ffffff0");
        let earlier_sha = v("0.1.0-dev.2+000000a");
        assert!(earlier_sha < later_sha);
    }

    #[test]
    fn full_version_joins_the_comparable_version_and_commit() {
        assert_eq!(
            full_version("0.1.0-dev.12", "abc1234"),
            "0.1.0-dev.12+abc1234"
        );
    }
}
