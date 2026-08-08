//! Installed packages: `SYNO.Core.Package`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::as_bool;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub id: String,
    /// DSM sends `name` for the human name and `id` for the short one.
    pub name: String,
    pub version: String,
    /// `running`, `stopped`, `installing`, …
    pub status: String,
    /// Present when the repository has something newer.
    pub available_version: Option<String>,
    pub beta: bool,
}

impl Package {
    pub fn is_running(&self) -> bool {
        self.status.eq_ignore_ascii_case("running")
    }

    /// Whether an update is waiting.
    ///
    /// Compared as strings deliberately: DSM version strings carry build
    /// numbers (`1.2.3-4567`) that do not parse as semver, and any ordering
    /// this invented would be wrong for some package. "Different from what is
    /// installed" is the honest test and matches what Package Center shows.
    pub fn has_update(&self) -> bool {
        match &self.available_version {
            Some(available) => !available.is_empty() && *available != self.version,
            None => false,
        }
    }

    pub fn list_from_json(data: &Value) -> Vec<Package> {
        let mut out: Vec<Package> = data
            .get("packages")
            .and_then(Value::as_array)
            .map(|list| list.iter().map(package_from).collect())
            .unwrap_or_default();
        // Case-insensitive, so "Antivirus" and "antivirus" sort together
        // rather than in two blocks either side of the alphabet.
        out.sort_by_key(|p| p.name.to_lowercase());
        out
    }
}

fn package_from(v: &Value) -> Package {
    let text = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let id = text("id");
    let name = {
        let display = text("name");
        if display.is_empty() {
            id.clone()
        } else {
            display
        }
    };
    // `status` is not a top-level field: it only appears under `additional`,
    // and only when the call asked for it. Reading it from the top level
    // silently yields "" and a page where nothing is ever running.
    let status = v
        .get("additional")
        .and_then(|a| a.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    Package {
        id,
        name,
        version: text("version"),
        status,
        available_version: v
            .get("available_version")
            .or_else(|| v.get("beta_version"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        beta: v.get("beta").and_then(as_bool).unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        // Verbatim shape from DSM 7.2.2: the human name is `name`, and
        // `status` is nested under `additional`.
        json!({"packages": [
            {"id": "ContainerManager", "name": "Container Manager",
             "version": "24.0.2-1606", "additional": {"status": "running"}},
            {"id": "SynologyPhotos", "name": "Synology Photos",
             "version": "1.7.0-0794", "additional": {"status": "running"},
             "available_version": "1.8.0-0801"},
            {"id": "AntiVirus", "name": "Antivirus Essential",
             "version": "1.4.6-0272", "additional": {"status": "stopped"}}
        ]})
    }

    #[test]
    fn packages_read_and_sort_by_name() {
        let ps = Package::list_from_json(&sample());
        assert_eq!(ps.len(), 3);
        assert_eq!(ps[0].name, "Antivirus Essential");
        assert_eq!(ps[1].name, "Container Manager");
        assert!(ps[1].is_running());
        assert!(!ps[0].is_running());
    }

    #[test]
    fn an_update_is_a_different_version_not_a_parsed_comparison() {
        // DSM versions like "1.7.0-0794" are not semver; any ordering we
        // invented would be wrong for some package.
        let ps = Package::list_from_json(&sample());
        let photos = ps
            .iter()
            .find(|p| p.id == "SynologyPhotos")
            .expect("photos");
        assert!(photos.has_update());

        let cm = ps.iter().find(|p| p.id == "ContainerManager").expect("cm");
        assert!(!cm.has_update());
    }

    #[test]
    fn an_available_version_equal_to_the_installed_one_is_not_an_update() {
        let ps = Package::list_from_json(&json!({"packages": [
            {"id": "x", "version": "1.0", "available_version": "1.0"}
        ]}));
        assert!(!ps[0].has_update());
    }

    #[test]
    fn a_package_with_no_display_name_falls_back_to_its_id() {
        let ps = Package::list_from_json(&json!({"packages": [{"id": "Bare"}]}));
        assert_eq!(ps[0].name, "Bare");
    }

    #[test]
    fn status_is_read_from_additional_where_dsm_actually_puts_it() {
        // There is no top-level `status`. Reading one gives "" for every
        // package and a page reporting that nothing is running.
        let ps = Package::list_from_json(&sample());
        assert!(ps.iter().any(|p| p.is_running()));
        assert_eq!(
            ps.iter().filter(|p| p.is_running()).count(),
            2,
            "two of the three sample packages are running"
        );
    }

    #[test]
    fn a_package_listed_without_the_status_addition_is_not_claimed_to_be_running() {
        let ps = Package::list_from_json(&json!({"packages": [{"id": "x", "name": "X"}]}));
        assert!(!ps[0].is_running());
    }

    #[test]
    fn an_empty_reply_is_an_empty_list() {
        assert!(Package::list_from_json(&json!({})).is_empty());
    }
}
