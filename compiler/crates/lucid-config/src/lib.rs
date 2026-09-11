//! Typed project manifest support for `project.yaml`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct Person {
    pub name: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct ProjectConfig {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub readme: Option<String>,
    #[serde(default)]
    pub dynamic: Vec<String>,
    pub lucid: Option<String>,
    pub license: Option<String>,
    #[serde(rename = "license-files", default)]
    pub license_files: Vec<String>,
    #[serde(default)]
    pub authors: Vec<Person>,
    #[serde(default)]
    pub maintainers: Vec<Person>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub classifiers: Vec<String>,
    #[serde(default)]
    pub urls: BTreeMap<String, String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(rename = "optional-dependencies", default)]
    pub optional_dependencies: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub export: BTreeMap<String, serde_yaml::Value>,
    #[serde(rename = "local-alias", default)]
    pub local_alias: BTreeMap<String, String>,
    #[serde(rename = "library-context")]
    pub library_context: Option<String>,
    #[serde(rename = "entry-points", default)]
    pub entry_points: BTreeMap<String, String>,
}

impl ProjectConfig {
    pub fn entry_point(&self, name: &str) -> Result<&str, String> {
        if !is_public_identifier(name) {
            return Err(format!("invalid entry point name '{name}'"));
        }
        let target = self
            .entry_points
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("unknown entry point '{name}'"))?;
        if !is_public_internal_path(target) {
            return Err(format!(
                "entry point '{name}' target '{target}' must be a public internal dotted path"
            ));
        }
        Ok(target)
    }

    /// Resolve a public dotted export path through the manifest's export
    /// trie. The returned target is the internal dotted path declared at the
    /// leaf, allowing loaders and packagers to share one traversal rule.
    pub fn export_target(&self, public_path: &str) -> Result<&str, String> {
        let mut segments = public_path.split('.');
        let first = segments
            .next()
            .filter(|segment| is_public_identifier(segment))
            .ok_or_else(|| format!("invalid public export path '{public_path}'"))?;
        let mut value = self
            .export
            .get(first)
            .ok_or_else(|| format!("unknown export path '{public_path}'"))?;
        for segment in segments {
            if !is_public_identifier(segment) {
                return Err(format!("invalid public export path '{public_path}'"));
            }
            let serde_yaml::Value::Mapping(entries) = value else {
                return Err(format!("export path '{public_path}' is not a leaf"));
            };
            value = entries
                .get(serde_yaml::Value::String(segment.to_owned()))
                .ok_or_else(|| format!("unknown export path '{public_path}'"))?;
        }
        match value {
            serde_yaml::Value::String(target) => Ok(target),
            serde_yaml::Value::Mapping(_) => Err(format!(
                "export path '{public_path}' names a namespace, not a symbol"
            )),
            _ => Err(format!("export path '{public_path}' has an invalid target")),
        }
    }

    /// Flatten every validated public export into a deterministic map from
    /// public dotted name to internal dotted target.
    pub fn resolved_exports(&self) -> Result<BTreeMap<String, String>, String> {
        self.validate()?;
        fn visit(
            node: &serde_yaml::Value,
            prefix: &str,
            result: &mut BTreeMap<String, String>,
        ) -> Result<(), String> {
            let serde_yaml::Value::Mapping(entries) = node else {
                return Err(format!("export '{prefix}' has an invalid shape"));
            };
            for (key, value) in entries {
                let key = key
                    .as_str()
                    .ok_or_else(|| format!("export '{prefix}' contains a non-string name"))?;
                let public_name = if prefix.is_empty() {
                    key.to_owned()
                } else {
                    format!("{prefix}.{key}")
                };
                match value {
                    serde_yaml::Value::String(target) => {
                        result.insert(public_name, target.clone());
                    }
                    serde_yaml::Value::Mapping(_) => visit(value, &public_name, result)?,
                    _ => return Err(format!("export '{public_name}' has an invalid shape")),
                }
            }
            Ok(())
        }
        let mut result = BTreeMap::new();
        for (name, tree) in &self.export {
            match tree {
                serde_yaml::Value::String(target) => {
                    result.insert(name.clone(), target.clone());
                }
                serde_yaml::Value::Mapping(_) => visit(tree, name, &mut result)?,
                _ => return Err(format!("export '{name}' has an invalid shape")),
            }
        }
        Ok(result)
    }

    /// Return the validated entry-point table in deterministic key order for
    /// command dispatch and package metadata generation.
    pub fn resolved_entry_points(&self) -> Result<BTreeMap<String, String>, String> {
        self.validate()?;
        Ok(self
            .entry_points
            .iter()
            .map(|(name, target)| (name.clone(), target.clone()))
            .collect())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().any(char::is_whitespace) {
            return Err("project name must be a non-empty name without whitespace".into());
        }
        let mut dynamic = std::collections::BTreeSet::new();
        for field in &self.dynamic {
            const DYNAMIC_FIELDS: &[&str] = &[
                "version",
                "description",
                "readme",
                "license",
                "license-files",
                "authors",
                "maintainers",
                "keywords",
                "classifiers",
                "urls",
                "dependencies",
                "optional-dependencies",
            ];
            if !DYNAMIC_FIELDS.contains(&field.as_str()) {
                return Err(format!("unknown dynamic project field '{field}'"));
            }
            if !dynamic.insert(field) {
                return Err(format!("duplicate dynamic field '{field}'"));
            }
        }
        for field in &dynamic {
            let supplied = match field.as_str() {
                "version" => self.version.is_some(),
                "description" => self.description.is_some(),
                "readme" => self.readme.is_some(),
                "license" => self.license.is_some(),
                "license-files" => !self.license_files.is_empty(),
                "authors" => !self.authors.is_empty(),
                "maintainers" => !self.maintainers.is_empty(),
                "keywords" => !self.keywords.is_empty(),
                "classifiers" => !self.classifiers.is_empty(),
                "urls" => !self.urls.is_empty(),
                "dependencies" => !self.dependencies.is_empty(),
                "optional-dependencies" => !self.optional_dependencies.is_empty(),
                _ => false,
            };
            if supplied {
                return Err(format!(
                    "dynamic field '{field}' must not also be provided statically"
                ));
            }
        }
        if self.version.is_none() && !dynamic.iter().any(|field| field.as_str() == "version") {
            return Err("project version is required unless declared dynamic".into());
        }
        if self
            .version
            .as_deref()
            .is_some_and(|version| version.trim().is_empty())
        {
            return Err("project version must not be empty".into());
        }
        for dependency in self.dependencies.keys() {
            if dependency.trim().is_empty() || dependency.chars().any(char::is_whitespace) {
                return Err("dependency name must be non-empty and contain no whitespace".into());
            }
        }
        for (dependency, requirement) in &self.dependencies {
            if requirement.trim().is_empty() {
                return Err(format!(
                    "dependency '{dependency}' version requirement must not be empty"
                ));
            }
        }
        for (group, dependencies) in &self.optional_dependencies {
            if group.trim().is_empty() || group.chars().any(char::is_whitespace) {
                return Err("optional dependency group name must not be empty".into());
            }
            for (dependency, requirement) in dependencies {
                if dependency.trim().is_empty()
                    || dependency.chars().any(char::is_whitespace)
                    || requirement.trim().is_empty()
                {
                    return Err(format!(
                        "optional dependency group '{group}' contains an empty name or requirement"
                    ));
                }
            }
        }
        for (name, target) in &self.entry_points {
            if name.trim().is_empty() || name.chars().any(char::is_whitespace) {
                return Err(format!(
                    "entry-point name '{name}' must be a non-empty name"
                ));
            }
            if !is_public_internal_path(target) {
                return Err(format!(
                    "entry point '{name}' target '{target}' must be a public internal dotted path"
                ));
            }
        }
        if let Some(context) = &self.library_context
            && context.trim().is_empty()
        {
            return Err("library-context must not be empty".into());
        }
        if let Some(context) = &self.library_context
            && !is_internal_path(context)
        {
            return Err("library-context must be an internal dotted path".into());
        }
        for (alias, target) in &self.local_alias {
            if alias.trim().is_empty()
                || alias.chars().any(char::is_whitespace)
                || target.trim().is_empty()
                || (target != "." && !is_internal_path(target))
            {
                return Err(
                    "local-alias names must be non-empty and targets must be internal paths".into(),
                );
            }
        }
        fn validate_export_node(
            node: &serde_yaml::Value,
            public_path: &mut Vec<String>,
        ) -> Result<(), String> {
            let serde_yaml::Value::Mapping(entries) = node else {
                return Err(format!(
                    "export path '{}' must contain a mapping or internal dotted target",
                    public_path.join(".")
                ));
            };
            for (key, value) in entries {
                let Some(key) = key.as_str() else {
                    return Err(format!(
                        "export path '{}' contains a non-string name",
                        public_path.join(".")
                    ));
                };
                if !is_public_identifier(key) {
                    return Err(format!("export name '{key}' must be a public identifier"));
                }
                public_path.push(key.to_owned());
                match value {
                    serde_yaml::Value::String(target) => {
                        if !is_internal_path(target)
                            || target
                                .trim_start_matches('.')
                                .split('.')
                                .any(|segment| segment.starts_with('_'))
                        {
                            return Err(format!(
                                "export '{}' target must be a public internal dotted path",
                                public_path.join(".")
                            ));
                        }
                    }
                    serde_yaml::Value::Mapping(_) => validate_export_node(value, public_path)?,
                    _ => {
                        return Err(format!(
                            "export '{}' must map to a target or nested mapping",
                            public_path.join(".")
                        ));
                    }
                }
                public_path.pop();
            }
            Ok(())
        }
        fn validate_export_target(public_path: &[String], target: &str) -> Result<(), String> {
            if !is_internal_path(target)
                || target
                    .trim_start_matches('.')
                    .split('.')
                    .any(|segment| segment.starts_with('_'))
            {
                return Err(format!(
                    "export '{}' target must be a public internal dotted path",
                    public_path.join(".")
                ));
            }
            Ok(())
        }
        for (name, tree) in &self.export {
            let mut path = vec![name.clone()];
            if !is_public_identifier(name) {
                return Err(format!("export name '{name}' must be a public identifier"));
            }
            match tree {
                serde_yaml::Value::String(target) => validate_export_target(&path, target)?,
                serde_yaml::Value::Mapping(_) => validate_export_node(tree, &mut path)?,
                _ => {
                    return Err(format!(
                        "export '{}' must map to a target or nested mapping",
                        path.join(".")
                    ));
                }
            }
        }
        Ok(())
    }
}

fn is_public_identifier(name: &str) -> bool {
    !name.trim().is_empty()
        && !name.starts_with('_')
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_internal_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('.') else {
        return false;
    };
    !rest.is_empty()
        && rest.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
                && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        })
}

fn is_public_internal_path(path: &str) -> bool {
    is_internal_path(path)
        && path
            .trim_start_matches('.')
            .split('.')
            .all(|segment| !segment.starts_with('_'))
}

pub fn parse_str(source: &str) -> Result<ProjectConfig, serde_yaml::Error> {
    serde_yaml::from_str(source)
}

pub fn load(path: impl AsRef<Path>) -> Result<ProjectConfig, LoadError> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(LoadError::Io)?;
    let config = parse_str(&source).map_err(LoadError::Yaml)?;
    config.validate().map_err(LoadError::Invalid)?;
    Ok(config)
}

/// Find the nearest `project.yaml` governing a source path. A source file may
/// live below the project root (for example in `src/package/module.lucid`),
/// so callers must search ancestors rather than checking only its immediate
/// parent directory.
pub fn find_project_manifest(path: impl AsRef<Path>) -> Option<std::path::PathBuf> {
    let path = path.as_ref();
    let start = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    start
        .ancestors()
        .map(|dir| dir.join("project.yaml"))
        .find(|candidate| candidate.is_file())
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct DevelopmentConfig {
    #[serde(default)]
    pub tools: BTreeMap<String, serde_yaml::Value>,
    #[serde(rename = "dependency-groups", default)]
    pub dependency_groups: BTreeMap<String, Vec<DependencySpec>>,
}

impl DevelopmentConfig {
    pub fn validate(&self) -> Result<(), String> {
        for tool in self.tools.keys() {
            if tool.trim().is_empty() || tool.chars().any(char::is_whitespace) {
                return Err("tool names must be non-empty and contain no whitespace".into());
            }
        }
        for (group, entries) in &self.dependency_groups {
            if group.trim().is_empty() || group.chars().any(char::is_whitespace) {
                return Err(
                    "dependency group names must be non-empty and contain no whitespace".into(),
                );
            }
            for entry in entries {
                match entry {
                    DependencySpec::IncludeGroup { include_group } => {
                        if include_group.trim().is_empty()
                            || include_group.chars().any(char::is_whitespace)
                        {
                            return Err(format!(
                                "dependency group '{group}' includes an invalid group name"
                            ));
                        }
                        if !self.dependency_groups.contains_key(include_group) {
                            return Err(format!(
                                "dependency group '{}' includes missing group '{}'",
                                group, include_group
                            ));
                        }
                    }
                    DependencySpec::Requirement(requirements) => {
                        for (name, version) in requirements {
                            if name.trim().is_empty()
                                || name.chars().any(char::is_whitespace)
                                || version.trim().is_empty()
                            {
                                return Err(format!(
                                    "dependency group '{group}' contains an empty name, whitespace, or requirement"
                                ));
                            }
                        }
                    }
                }
            }
        }
        let mut state = BTreeMap::<&str, u8>::new();
        fn visit<'a>(
            group: &'a str,
            config: &'a DevelopmentConfig,
            state: &mut BTreeMap<&'a str, u8>,
        ) -> Result<(), String> {
            match state.get(group).copied().unwrap_or(0) {
                2 => return Ok(()),
                1 => return Err(format!("cyclic dependency group involving '{group}'")),
                _ => {}
            }
            state.insert(group, 1);
            if let Some(entries) = config.dependency_groups.get(group) {
                for entry in entries {
                    if let DependencySpec::IncludeGroup { include_group } = entry {
                        visit(include_group, config, state)?;
                    }
                }
            }
            state.insert(group, 2);
            Ok(())
        }
        for group in self.dependency_groups.keys() {
            visit(group, self, &mut state)?;
        }
        Ok(())
    }

    pub fn expanded_group(&self, group: &str) -> Result<BTreeMap<String, String>, String> {
        self.validate()?;
        if !self.dependency_groups.contains_key(group) {
            return Err(format!("unknown dependency group '{group}'"));
        }
        fn expand(
            group: &str,
            config: &DevelopmentConfig,
            result: &mut BTreeMap<String, String>,
        ) -> Result<(), String> {
            for entry in config.dependency_groups.get(group).into_iter().flatten() {
                match entry {
                    DependencySpec::IncludeGroup { include_group } => {
                        expand(include_group, config, result)?;
                    }
                    DependencySpec::Requirement(requirements) => {
                        for (name, version) in requirements {
                            if let Some(previous) = result.get(name) {
                                if previous != version {
                                    return Err(format!(
                                        "conflicting requirements for '{name}': '{previous}' and '{version}'"
                                    ));
                                }
                            } else {
                                result.insert(name.clone(), version.clone());
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        let mut result = BTreeMap::new();
        expand(group, self, &mut result)?;
        Ok(result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    IncludeGroup {
        #[serde(rename = "include-group")]
        include_group: String,
    },
    Requirement(BTreeMap<String, String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct LockFile {
    #[serde(default)]
    pub package: Vec<LockedPackage>,
}

impl LockFile {
    /// Validate the graph that an installer would use for deterministic
    /// dependency initialization.
    pub fn validate(&self) -> Result<(), String> {
        let mut indices = BTreeMap::new();
        for (index, package) in self.package.iter().enumerate() {
            if package.name.trim().is_empty()
                || package.name.chars().any(char::is_whitespace)
                || package.version.trim().is_empty()
                || package.version.chars().any(char::is_whitespace)
            {
                return Err(
                    "locked package name and version must be non-empty and contain no whitespace"
                        .into(),
                );
            }
            if indices.insert(package.name.as_str(), index).is_some() {
                return Err(format!("duplicate locked package '{}'", package.name));
            }
        }
        for package in &self.package {
            let digest = package.hash.strip_prefix("sha256:");
            if digest
                .is_none_or(|digest| digest.is_empty() || digest.chars().any(char::is_whitespace))
            {
                return Err(format!(
                    "locked package '{}' has an invalid content hash",
                    package.name
                ));
            }
            for dependency in &package.dependencies {
                if dependency.trim().is_empty() || dependency.chars().any(char::is_whitespace) {
                    return Err(format!(
                        "locked package '{}' has an invalid dependency name",
                        package.name
                    ));
                }
                if !indices.contains_key(dependency.as_str()) {
                    return Err(format!(
                        "locked package '{}' refers to missing dependency '{}'",
                        package.name, dependency
                    ));
                }
            }
        }
        let mut state = vec![0u8; self.package.len()];
        fn visit(
            index: usize,
            packages: &[LockedPackage],
            indices: &BTreeMap<&str, usize>,
            state: &mut [u8],
        ) -> Result<(), String> {
            if state[index] == 2 {
                return Ok(());
            }
            if state[index] == 1 {
                return Err(format!(
                    "cyclic locked dependency involving '{}'",
                    packages[index].name
                ));
            }
            state[index] = 1;
            for dependency in &packages[index].dependencies {
                visit(indices[dependency.as_str()], packages, indices, state)?;
            }
            state[index] = 2;
            Ok(())
        }
        for index in 0..self.package.len() {
            visit(index, &self.package, &indices, &mut state)?;
        }
        Ok(())
    }

    pub fn install_order(&self) -> Result<Vec<String>, String> {
        self.validate()?;
        let indices = self
            .package
            .iter()
            .enumerate()
            .map(|(index, package)| (package.name.as_str(), index))
            .collect::<BTreeMap<_, _>>();
        let mut state = vec![0u8; self.package.len()];
        let mut order = Vec::with_capacity(self.package.len());
        fn visit(
            index: usize,
            packages: &[LockedPackage],
            indices: &BTreeMap<&str, usize>,
            state: &mut [u8],
            order: &mut Vec<String>,
        ) {
            if state[index] == 2 {
                return;
            }
            state[index] = 1;
            let mut dependencies = packages[index].dependencies.iter().collect::<Vec<_>>();
            dependencies.sort();
            for dependency in dependencies {
                visit(
                    indices[dependency.as_str()],
                    packages,
                    indices,
                    state,
                    order,
                );
            }
            state[index] = 2;
            order.push(packages[index].name.clone());
        }
        let mut roots = (0..self.package.len()).collect::<Vec<_>>();
        roots.sort_by(|left, right| self.package[*left].name.cmp(&self.package[*right].name));
        for index in roots {
            visit(index, &self.package, &indices, &mut state, &mut order);
        }
        Ok(order)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub hash: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

pub fn parse_development_str(source: &str) -> Result<DevelopmentConfig, serde_yaml::Error> {
    serde_yaml::from_str(source)
}

pub fn load_development(path: impl AsRef<Path>) -> Result<DevelopmentConfig, LoadError> {
    let source = std::fs::read_to_string(path).map_err(LoadError::Io)?;
    let config = parse_development_str(&source).map_err(LoadError::Yaml)?;
    config.validate().map_err(LoadError::Invalid)?;
    Ok(config)
}

pub fn parse_lock_str(source: &str) -> Result<LockFile, serde_yaml::Error> {
    serde_yaml::from_str(source)
}

pub fn load_lock(path: impl AsRef<Path>) -> Result<LockFile, LoadError> {
    let source = std::fs::read_to_string(path).map_err(LoadError::Io)?;
    let lock = parse_lock_str(&source).map_err(LoadError::Yaml)?;
    lock.validate().map_err(LoadError::Invalid)?;
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spec_manifest_keys_and_dependency_maps() {
        let config = parse_str(
            "name: acme-inference\nversion: \"2.3.0\"\nlucid: \">=0.4\"\nlicense-files: [LICENSE]\nauthors:\n  - name: Ada Lovelace\n    email: ada@example.com\ndependencies:\n  numpy: \">=1.26\"\noptional-dependencies:\n  gpu:\n    cupy: \">=13.0\"\nentry-points:\n  serve: .cli.serve\n",
        )
        .expect("manifest should parse");
        assert_eq!(config.name, "acme-inference");
        assert_eq!(config.version.as_deref(), Some("2.3.0"));
        assert_eq!(config.license_files, vec!["LICENSE"]);
        assert_eq!(config.authors[0].name, "Ada Lovelace");
        assert_eq!(config.dependencies["numpy"], ">=1.26");
        assert_eq!(config.optional_dependencies["gpu"]["cupy"], ">=13.0");
        assert_eq!(config.entry_points["serve"], ".cli.serve");
        assert_eq!(config.entry_point("serve"), Ok(".cli.serve"));
        assert_eq!(
            config.resolved_entry_points().unwrap(),
            BTreeMap::from([("serve".into(), ".cli.serve".into())])
        );
        assert!(config.entry_point("missing").is_err());
        assert!(
            config
                .entry_point("bad name")
                .unwrap_err()
                .contains("invalid")
        );
        config.validate().expect("valid project should validate");
    }

    #[test]
    fn rejects_manifest_without_required_name() {
        assert!(parse_str("version: '1.0'\n").is_err());
        let spaced = parse_str("name: 'my app'\nversion: '1.0'\n").unwrap();
        assert!(spaced.validate().unwrap_err().contains("whitespace"));
    }

    #[test]
    fn validates_dynamic_version_and_rejects_duplicate_fields() {
        let dynamic = parse_str("name: app\ndynamic: [version]\n").unwrap();
        dynamic
            .validate()
            .expect("dynamic version should be allowed");
        let duplicate =
            parse_str("name: app\nversion: '1'\ndynamic: [version, version]\n").unwrap();
        assert!(duplicate.validate().unwrap_err().contains("duplicate"));
        let empty = parse_str("name: app\nversion: ' '\n").unwrap();
        assert!(empty.validate().unwrap_err().contains("version"));
        let unknown = parse_str("name: app\ndynamic: [versoin]\n").unwrap();
        assert!(unknown.validate().unwrap_err().contains("unknown dynamic"));
        let conflicting = parse_str("name: app\nversion: '1'\ndynamic: [version]\n").unwrap();
        assert!(conflicting.validate().unwrap_err().contains("statically"));
    }

    #[test]
    fn rejects_empty_entry_point_targets() {
        let config = parse_str("name: app\nversion: '1'\nentry-points:\n  run: '  '\n").unwrap();
        assert!(config.validate().unwrap_err().contains("target"));
        let external =
            parse_str("name: app\nversion: '1'\nentry-points:\n  run: cli.serve\n").unwrap();
        assert!(external.validate().unwrap_err().contains("dotted path"));
        assert!(
            external
                .entry_point("run")
                .unwrap_err()
                .contains("dotted path")
        );
        let private =
            parse_str("name: app\nversion: '1'\nentry-points:\n  run: ._internal.run\n").unwrap();
        assert!(private.validate().unwrap_err().contains("public"));
    }

    #[test]
    fn validates_export_tree_shape_and_public_targets() {
        let valid = parse_str(
            "name: app\nversion: '1'\nexport:\n  models:\n    User: .models.User\n  run: .cli.run\n",
        )
        .unwrap();
        valid
            .validate()
            .expect("nested and direct exports are valid");
        assert_eq!(valid.export_target("models.User"), Ok(".models.User"));
        assert_eq!(valid.export_target("run"), Ok(".cli.run"));
        assert_eq!(
            valid.resolved_exports().unwrap(),
            BTreeMap::from([
                ("models.User".into(), ".models.User".into()),
                ("run".into(), ".cli.run".into()),
            ])
        );
        assert!(
            valid
                .export_target("models")
                .unwrap_err()
                .contains("namespace")
        );
        assert!(valid.export_target("missing").is_err());
        for source in [
            "name: app\nversion: '1'\nexport:\n  _private: .models.User\n",
            "name: app\nversion: '1'\nexport:\n  models:\n    User: ._models.User\n",
            "name: app\nversion: '1'\nexport:\n  models:\n    1User: .models.User\n",
            "name: app\nversion: '1'\nexport:\n  models: [User]\n",
        ] {
            let config = parse_str(source).unwrap();
            assert!(
                config.validate().is_err(),
                "invalid export should fail: {source}"
            );
        }
    }

    #[test]
    fn validates_internal_library_context_and_local_alias_paths() {
        let valid = parse_str(
            "name: app\nversion: '1'\nlibrary-context: .setup.initialize\nlocal-alias:\n  src: .\n  matrix: .models.matrix\n",
        )
        .unwrap();
        valid.validate().expect("internal paths should validate");
        let external =
            parse_str("name: app\nversion: '1'\nlibrary-context: setup.initialize\n").unwrap();
        assert!(external.validate().unwrap_err().contains("library-context"));
        let bad_alias =
            parse_str("name: app\nversion: '1'\nlocal-alias:\n  bad key: .target\n").unwrap();
        assert!(bad_alias.validate().unwrap_err().contains("local-alias"));
    }

    #[test]
    fn rejects_empty_dependency_requirements() {
        let config = parse_str("name: app\nversion: '1'\ndependencies:\n  dep: ' '\n").unwrap();
        assert!(config.validate().unwrap_err().contains("requirement"));
        let group = parse_str(
            "name: app\nversion: '1'\noptional-dependencies:\n  'bad group':\n    dep: '>=1'\n",
        )
        .unwrap();
        assert!(
            group
                .validate()
                .unwrap_err()
                .contains("optional dependency group")
        );
    }

    #[test]
    fn parses_development_groups_and_includes() {
        let config = parse_development_str(
            "dependency-groups:\n  test:\n    - pytest: \">=8\"\n  dev:\n    - include-group: test\n",
        )
        .expect("development config should parse");
        assert!(config.dependency_groups.contains_key("dev"));
        assert!(matches!(
            config.dependency_groups["dev"][0],
            DependencySpec::IncludeGroup { ref include_group } if include_group == "test"
        ));
        config.validate().expect("valid groups should validate");
        assert_eq!(config.expanded_group("dev").unwrap()["pytest"], ">=8");
    }

    #[test]
    fn parses_lockfile_packages_and_hashes() {
        let lock = parse_lock_str(
            "package:\n  - name: numpy\n    version: \"1.26.4\"\n    hash: \"sha256:abc\"\n    dependencies: []\n",
        )
        .expect("lockfile should parse");
        assert_eq!(lock.package[0].name, "numpy");
        assert_eq!(lock.package[0].hash, "sha256:abc");
        lock.validate().expect("valid lock graph should validate");
        assert_eq!(lock.install_order().unwrap(), vec!["numpy"]);
        let ordered = parse_lock_str(
            "package:\n  - {name: app, version: '1', hash: sha256:a, dependencies: [numpy]}\n  - {name: numpy, version: '1', hash: sha256:n}\n",
        )
        .unwrap();
        assert_eq!(ordered.install_order().unwrap(), vec!["numpy", "app"]);
        let reversed = parse_lock_str(
            "package:\n  - {name: zed, version: '1', hash: sha256:z}\n  - {name: alpha, version: '1', hash: sha256:a}\n",
        )
        .unwrap();
        assert_eq!(reversed.install_order().unwrap(), vec!["alpha", "zed"]);
    }

    #[test]
    fn rejects_missing_and_cyclic_dependency_groups() {
        let missing =
            parse_development_str("dependency-groups:\n  dev:\n    - include-group: missing\n")
                .unwrap();
        assert!(missing.validate().unwrap_err().contains("missing group"));
        let cycle = parse_development_str(
            "dependency-groups:\n  a:\n    - include-group: b\n  b:\n    - include-group: a\n",
        )
        .unwrap();
        assert!(cycle.validate().unwrap_err().contains("cyclic"));
    }

    #[test]
    fn rejects_malformed_dependency_group_entries() {
        for source in [
            "dependency-groups:\n  dev:\n    - {'bad name': '>=1'}\n",
            "dependency-groups:\n  dev:\n    - {pkg: ''}\n",
            "dependency-groups:\n  dev:\n    - include-group: 'bad group'\n",
            "dependency-groups:\n  dev:\n    - include-group: ''\n",
            "dependency-groups:\n  bad group:\n    - pkg: '>=1'\n",
        ] {
            let config = parse_development_str(source).unwrap();
            assert!(
                config.validate().is_err(),
                "malformed dependency entry should be rejected: {source}"
            );
        }
        for source in ["tools:\n  'bad tool': {}\n", "tools:\n  '': {}\n"] {
            let config = parse_development_str(source).unwrap();
            assert!(
                config.validate().is_err(),
                "malformed tool should fail: {source}"
            );
        }
    }

    #[test]
    fn rejects_conflicting_group_requirements() {
        let config = parse_development_str(
            "dependency-groups:\n  a:\n    - {pkg: '>=1'}\n  b:\n    - {pkg: '>=2'}\n  all:\n    - include-group: a\n    - include-group: b\n",
        )
        .unwrap();
        assert!(
            config
                .expanded_group("all")
                .unwrap_err()
                .contains("conflicting")
        );
    }

    #[test]
    fn load_rejects_semantically_invalid_manifest() {
        let path =
            std::env::temp_dir().join(format!("lucid_config_invalid_{}", std::process::id()));
        std::fs::write(&path, "name: app\n").unwrap();
        let result = load(&path);
        let _ = std::fs::remove_file(&path);
        assert!(matches!(result, Err(LoadError::Invalid(message)) if message.contains("version")));
    }

    #[test]
    fn file_loaders_validate_development_and_lock_files() {
        let base =
            std::env::temp_dir().join(format!("lucid_config_loaders_{}", std::process::id()));
        let development_path = base.with_extension("yaml");
        let lock_path = base.with_extension("lock");
        std::fs::write(
            &development_path,
            "dependency-groups:\n  test:\n    - pytest: '>=8'\n",
        )
        .unwrap();
        std::fs::write(
            &lock_path,
            "package:\n  - {name: pytest, version: '8', hash: sha256:test}\n",
        )
        .unwrap();
        assert!(load_development(&development_path).is_ok());
        assert!(load_lock(&lock_path).is_ok());
        let _ = std::fs::remove_file(development_path);
        let _ = std::fs::remove_file(lock_path);
    }

    #[test]
    fn lock_loader_rejects_invalid_hashes() {
        let path =
            std::env::temp_dir().join(format!("lucid_config_bad_lock_{}", std::process::id()));
        std::fs::write(
            &path,
            "package:\n  - {name: pkg, version: '1', hash: bad}\n",
        )
        .unwrap();
        let result = load_lock(&path);
        let _ = std::fs::remove_file(path);
        assert!(
            matches!(result, Err(LoadError::Invalid(message)) if message.contains("content hash"))
        );
        let whitespace =
            parse_lock_str("package:\n  - {name: pkg, version: '1', hash: 'sha256:abc def'}\n")
                .unwrap();
        assert!(whitespace.validate().unwrap_err().contains("content hash"));
    }

    #[test]
    fn rejects_invalid_lock_graphs() {
        let duplicate = parse_lock_str(
            "package:\n  - {name: a, version: '1', hash: h}\n  - {name: a, version: '2', hash: h}\n",
        )
        .unwrap();
        assert!(duplicate.validate().unwrap_err().contains("duplicate"));

        let cycle = parse_lock_str(
            "package:\n  - {name: a, version: '1', hash: sha256:h, dependencies: [b]}\n  - {name: b, version: '1', hash: sha256:h, dependencies: [a]}\n",
        )
        .unwrap();
        assert!(cycle.validate().unwrap_err().contains("cyclic"));

        let invalid_hash =
            parse_lock_str("package:\n  - {name: a, version: '1', hash: unknown}\n").unwrap();
        assert!(
            invalid_hash
                .validate()
                .unwrap_err()
                .contains("content hash")
        );

        let empty_identity =
            parse_lock_str("package:\n  - {name: '', version: '1', hash: sha256:a}\n").unwrap();
        assert!(
            empty_identity
                .validate()
                .unwrap_err()
                .contains("name and version")
        );

        let whitespace_name =
            parse_lock_str("package:\n  - {name: 'bad name', version: '1', hash: sha256:a}\n")
                .unwrap();
        assert!(
            whitespace_name
                .validate()
                .unwrap_err()
                .contains("whitespace")
        );

        let whitespace_dependency = parse_lock_str(
            "package:\n  - {name: a, version: '1', hash: sha256:a, dependencies: ['bad name']}\n",
        )
        .unwrap();
        assert!(
            whitespace_dependency
                .validate()
                .unwrap_err()
                .contains("dependency name")
        );
    }

    #[test]
    fn finds_manifest_for_nested_source_path() {
        let root = std::env::temp_dir().join(format!(
            "lucid_config_manifest_search_{}",
            std::process::id()
        ));
        let nested = root.join("src/pkg");
        std::fs::create_dir_all(&nested).unwrap();
        let manifest = root.join("project.yaml");
        let source = nested.join("module.lucid");
        std::fs::write(&manifest, "name: app\n").unwrap();
        std::fs::write(&source, "").unwrap();
        assert_eq!(find_project_manifest(&source), Some(manifest.clone()));
        let _ = std::fs::remove_dir_all(root);
    }
}
