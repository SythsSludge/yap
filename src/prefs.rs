//! Matching preferences: what you are and who you're looking for.

use crate::catalog::{ANY, Catalog};
use crate::protocol::{WirePartner, WirePreferences, WireUser};
use serde::{Deserialize, Serialize};

/// Every preference field, in the order the web client shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Gender,
    Species,
    Role,
    PartnerGender,
    PartnerSpecies,
    PartnerRole,
    Kinks,
    Language,
}

impl Field {
    pub const ALL: [Field; 8] = [
        Field::Gender,
        Field::Species,
        Field::Role,
        Field::PartnerGender,
        Field::PartnerSpecies,
        Field::PartnerRole,
        Field::Kinks,
        Field::Language,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Gender => "Your gender",
            Field::Species => "Your species",
            Field::Role => "Your role",
            Field::PartnerGender => "Partner's gender",
            Field::PartnerSpecies => "Partner's species",
            Field::PartnerRole => "Partner's role",
            Field::Kinks => "Kinks",
            Field::Language => "Language",
        }
    }

    pub fn catalog(self) -> Catalog {
        match self {
            Field::Gender | Field::PartnerGender => Catalog::Gender,
            Field::Species | Field::PartnerSpecies => Catalog::Species,
            Field::Role | Field::PartnerRole => Catalog::Role,
            Field::Kinks => Catalog::Kinks,
            Field::Language => Catalog::Language,
        }
    }

    pub fn is_multi(self) -> bool {
        matches!(self, Field::PartnerGender | Field::PartnerSpecies | Field::Kinks)
    }

    /// Whether "Any / All" is offered for this field.
    pub fn allows_any(self) -> bool {
        self.is_multi() || self == Field::Language
    }
}

/// A full set of preferences. Single-choice fields are `None` until picked; multi-choice
/// fields hold either `["any"]` or a list of concrete values, never both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub gender: Option<String>,
    pub species: Option<String>,
    pub role: Option<String>,
    pub partner_gender: Vec<String>,
    pub partner_species: Vec<String>,
    pub partner_role: Option<String>,
    pub kinks: Vec<String>,
    pub language: String,
}

impl Default for Preferences {
    /// Matches a fresh web client: multi-selects start on "Any / All".
    fn default() -> Self {
        Preferences {
            gender: None,
            species: None,
            role: None,
            partner_gender: vec![ANY.into()],
            partner_species: vec![ANY.into()],
            partner_role: None,
            kinks: vec![ANY.into()],
            language: ANY.into(),
        }
    }
}

/// Why preferences can't be submitted. Messages match the web client's toasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error("Please select your gender.")]
    Gender,
    #[error("Please select your species.")]
    Species,
    #[error("Please select your role.")]
    Role,
    #[error("Please select the gender you're seeking.")]
    PartnerGender,
    #[error("Please select the species you're seeking.")]
    PartnerSpecies,
    #[error("Please select the role you're seeking.")]
    PartnerRole,
    #[error("Please select the kinks you're interested in.")]
    Kinks,
    #[error("Please select a preferred language.")]
    Language,
}

impl Invalid {
    pub fn field(self) -> Field {
        match self {
            Invalid::Gender => Field::Gender,
            Invalid::Species => Field::Species,
            Invalid::Role => Field::Role,
            Invalid::PartnerGender => Field::PartnerGender,
            Invalid::PartnerSpecies => Field::PartnerSpecies,
            Invalid::PartnerRole => Field::PartnerRole,
            Invalid::Kinks => Field::Kinks,
            Invalid::Language => Field::Language,
        }
    }
}

impl Preferences {
    /// The selected values for a field, as a list (single fields yield 0 or 1 items).
    pub fn values(&self, field: Field) -> Vec<&str> {
        fn single(v: &Option<String>) -> Vec<&str> {
            v.iter().map(String::as_str).collect()
        }
        fn multi(v: &[String]) -> Vec<&str> {
            v.iter().map(String::as_str).collect()
        }
        match field {
            Field::Gender => single(&self.gender),
            Field::Species => single(&self.species),
            Field::Role => single(&self.role),
            Field::PartnerRole => single(&self.partner_role),
            Field::PartnerGender => multi(&self.partner_gender),
            Field::PartnerSpecies => multi(&self.partner_species),
            Field::Kinks => multi(&self.kinks),
            Field::Language => vec![self.language.as_str()],
        }
    }

    pub fn is_selected(&self, field: Field, value: &str) -> bool {
        self.values(field).contains(&value)
    }

    /// Choose `value` for `field`. Single fields are replaced; multi fields toggle the
    /// value. Picking "any" clears concrete picks and vice versa, since the server
    /// treats `any` as a wildcard that makes other picks meaningless.
    pub fn toggle(&mut self, field: Field, value: &str) {
        if value == ANY && !field.allows_any() {
            return;
        }
        if value != ANY && !field.catalog().contains(value) {
            return;
        }
        match field {
            Field::Gender => self.gender = Some(value.into()),
            Field::Species => self.species = Some(value.into()),
            Field::Role => self.role = Some(value.into()),
            Field::PartnerRole => self.partner_role = Some(value.into()),
            Field::Language => self.language = value.into(),
            Field::PartnerGender => toggle_multi(&mut self.partner_gender, value),
            Field::PartnerSpecies => toggle_multi(&mut self.partner_species, value),
            Field::Kinks => toggle_multi(&mut self.kinks, value),
        }
    }

    /// Reset a multi field to "Any / All", or clear a single field.
    pub fn clear(&mut self, field: Field) {
        match field {
            Field::Gender => self.gender = None,
            Field::Species => self.species = None,
            Field::Role => self.role = None,
            Field::PartnerRole => self.partner_role = None,
            Field::Language => self.language = ANY.into(),
            Field::PartnerGender => self.partner_gender = vec![ANY.into()],
            Field::PartnerSpecies => self.partner_species = vec![ANY.into()],
            Field::Kinks => self.kinks = vec![ANY.into()],
        }
    }

    /// Validate in the same order as the web client, returning the first problem.
    pub fn validate(&self) -> Result<(), Invalid> {
        let single_ok = |v: &Option<String>, cat: Catalog| v.as_deref().is_some_and(|v| cat.contains(v));
        let multi_ok = |v: &Vec<String>, cat: Catalog| !v.is_empty() && v.iter().all(|x| x == ANY || cat.contains(x));
        if !single_ok(&self.gender, Catalog::Gender) {
            return Err(Invalid::Gender);
        }
        if !single_ok(&self.species, Catalog::Species) {
            return Err(Invalid::Species);
        }
        if !single_ok(&self.role, Catalog::Role) {
            return Err(Invalid::Role);
        }
        if !multi_ok(&self.partner_gender, Catalog::Gender) {
            return Err(Invalid::PartnerGender);
        }
        if !multi_ok(&self.partner_species, Catalog::Species) {
            return Err(Invalid::PartnerSpecies);
        }
        if !single_ok(&self.partner_role, Catalog::Role) {
            return Err(Invalid::PartnerRole);
        }
        if !multi_ok(&self.kinks, Catalog::Kinks) {
            return Err(Invalid::Kinks);
        }
        if self.language != ANY && !Catalog::Language.contains(&self.language) {
            return Err(Invalid::Language);
        }
        Ok(())
    }

    /// Build the `find_partner` payload. `send_language` exists because the live site
    /// predates the language field.
    pub fn to_wire(&self, send_language: bool) -> Result<WirePreferences, Invalid> {
        self.validate()?;
        let get = |v: &Option<String>| v.clone().expect("validated");
        Ok(WirePreferences {
            user: WireUser {
                gender: get(&self.gender),
                species: get(&self.species),
                role: get(&self.role),
                language: send_language.then(|| self.language.clone()),
            },
            partner: WirePartner {
                gender: self.partner_gender.clone(),
                species: self.partner_species.clone(),
                role: get(&self.partner_role),
            },
            kinks: self.kinks.clone(),
        })
    }

    /// Drop values the server wouldn't accept (e.g. from an old or hand-edited file) and
    /// restore multi-field invariants. Returns the values that were removed.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut dropped = Vec::new();
        let mut single = |v: &mut Option<String>, cat: Catalog| {
            if let Some(x) = v.as_ref().filter(|x| !cat.contains(x)) {
                dropped.push(x.clone());
                *v = None;
            }
        };
        single(&mut self.gender, Catalog::Gender);
        single(&mut self.species, Catalog::Species);
        single(&mut self.role, Catalog::Role);
        single(&mut self.partner_role, Catalog::Role);
        if self.language != ANY && !Catalog::Language.contains(&self.language) {
            dropped.push(std::mem::replace(&mut self.language, ANY.into()));
        }
        for (list, cat) in [
            (&mut self.partner_gender, Catalog::Gender),
            (&mut self.partner_species, Catalog::Species),
            (&mut self.kinks, Catalog::Kinks),
        ] {
            let mut seen = std::collections::HashSet::new();
            list.retain(|x| {
                let ok = (x == ANY || cat.contains(x)) && seen.insert(x.clone());
                if !ok {
                    dropped.push(x.clone());
                }
                ok
            });
            if list.is_empty() || (list.len() > 1 && list.iter().any(|x| x == ANY)) {
                *list = vec![ANY.into()];
            }
        }
        dropped
    }

    /// Import from the web client's `localStorage`, as produced by running
    /// `copy(JSON.stringify(localStorage))` in the browser console on yiffspot.com.
    /// Multi-selects are stored there as comma-joined strings.
    pub fn from_web_local_storage(json: &serde_json::Value) -> Option<Self> {
        let obj = json.as_object()?;
        let known = ["gender", "species", "role", "partnerGender", "partnerSpecies", "partnerRole", "kinks"];
        if !known.iter().any(|k| obj.contains_key(*k)) {
            return None;
        }
        let str_of = |k: &str| obj.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty());
        let list_of = |k: &str| {
            str_of(k).map(|s| s.split(',').map(str::to_owned).collect::<Vec<_>>()).unwrap_or_else(|| vec![ANY.into()])
        };
        let mut prefs = Preferences {
            gender: str_of("gender").map(Into::into),
            species: str_of("species").map(Into::into),
            role: str_of("role").map(Into::into),
            partner_gender: list_of("partnerGender"),
            partner_species: list_of("partnerSpecies"),
            partner_role: str_of("partnerRole").map(Into::into),
            kinks: list_of("kinks"),
            language: str_of("language").unwrap_or(ANY).into(),
        };
        prefs.sanitize();
        Some(prefs)
    }

    /// Short human summary of a field's value for list views.
    pub fn summary(&self, field: Field) -> String {
        let values = self.values(field);
        match values.as_slice() {
            [] => "—".into(),
            [v] if *v == ANY => "Any / All".into(),
            [v] => (*v).into(),
            [first, rest @ ..] => format!("{first} +{}", rest.len()),
        }
    }
}

fn toggle_multi(list: &mut Vec<String>, value: &str) {
    if value == ANY {
        *list = vec![ANY.into()];
        return;
    }
    list.retain(|x| x != ANY);
    if let Some(pos) = list.iter().position(|x| x == value) {
        list.remove(pos);
        if list.is_empty() {
            list.push(ANY.into());
        }
    } else {
        list.push(value.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    pub(crate) fn complete() -> Preferences {
        let mut p = Preferences::default();
        p.toggle(Field::Gender, "Male");
        p.toggle(Field::Species, "Wolf");
        p.toggle(Field::Role, "Switch");
        p.toggle(Field::PartnerRole, "Dominant");
        p
    }

    #[test]
    fn defaults_match_fresh_web_client() {
        let p = Preferences::default();
        assert_eq!(p.partner_gender, vec!["any"]);
        assert_eq!(p.kinks, vec!["any"]);
        assert_eq!(p.language, "any");
        assert_eq!(p.validate(), Err(Invalid::Gender));
    }

    #[test]
    fn validation_reports_first_missing_field_in_web_order() {
        let mut p = Preferences::default();
        assert_eq!(p.validate(), Err(Invalid::Gender));
        p.toggle(Field::Gender, "Male");
        assert_eq!(p.validate(), Err(Invalid::Species));
        p.toggle(Field::Species, "Fox");
        assert_eq!(p.validate(), Err(Invalid::Role));
        p.toggle(Field::Role, "Dominant");
        assert_eq!(p.validate(), Err(Invalid::PartnerRole));
        p.toggle(Field::PartnerRole, "Submissive");
        assert_eq!(p.validate(), Ok(()));
        assert_eq!(Invalid::PartnerRole.to_string(), "Please select the role you're seeking.");
    }

    #[test]
    fn multi_toggle_keeps_any_exclusive() {
        let mut p = complete();
        p.toggle(Field::Kinks, "Biting");
        assert_eq!(p.kinks, vec!["Biting"]);
        p.toggle(Field::Kinks, "Anal");
        assert_eq!(p.kinks, vec!["Biting", "Anal"]);
        p.toggle(Field::Kinks, "Biting");
        assert_eq!(p.kinks, vec!["Anal"]);
        // Removing the last concrete pick falls back to Any rather than an empty list,
        // which the server would reject.
        p.toggle(Field::Kinks, "Anal");
        assert_eq!(p.kinks, vec!["any"]);
        p.toggle(Field::Kinks, "Anal");
        p.toggle(Field::Kinks, ANY);
        assert_eq!(p.kinks, vec!["any"]);
    }

    #[test]
    fn toggle_ignores_values_the_server_would_reject() {
        let mut p = complete();
        p.toggle(Field::Species, "Not A Species");
        assert_eq!(p.species.as_deref(), Some("Wolf"));
        p.toggle(Field::Gender, ANY);
        assert_eq!(p.gender.as_deref(), Some("Male"));
        p.toggle(Field::Kinks, "Wolf");
        assert_eq!(p.kinks, vec!["any"]);
    }

    #[test]
    fn to_wire_builds_find_partner_payload() {
        let mut p = complete();
        p.toggle(Field::PartnerSpecies, "Fox");
        let wire = p.to_wire(true).unwrap();
        assert_eq!(wire.user.gender, "Male");
        assert_eq!(wire.user.language.as_deref(), Some("any"));
        assert_eq!(wire.partner.species, vec!["Fox"]);
        assert_eq!(wire.partner.role, "Dominant");
        assert_eq!(p.to_wire(false).unwrap().user.language, None);
        assert_eq!(Preferences::default().to_wire(true), Err(Invalid::Gender));
    }

    #[test]
    fn sanitize_drops_unknown_values_and_fixes_invariants() {
        let mut p = Preferences {
            gender: Some("Robot".into()),
            partner_species: vec!["any".into(), "Fox".into()],
            kinks: vec!["Anal".into(), "Nope".into(), "Anal".into()],
            language: "Klingon".into(),
            ..complete()
        };
        let dropped = p.sanitize();
        assert_eq!(p.gender, None);
        assert_eq!(p.partner_species, vec!["any"]);
        assert_eq!(p.kinks, vec!["Anal"]);
        assert_eq!(p.language, "any");
        assert!(dropped.contains(&"Robot".to_string()));
        assert!(dropped.contains(&"Nope".to_string()));
        assert!(dropped.contains(&"Klingon".to_string()));
    }

    #[test]
    fn imports_web_local_storage_dump() {
        let dump = json!({
            "user": "{\"id\":\"123\",\"hasPartner\":false}",
            "gender": "Female",
            "species": "Snow Leopard",
            "role": "Submissive",
            "partnerGender": "Male,Female",
            "partnerSpecies": "any",
            "partnerRole": "Dominant",
            "kinks": "Biting,Musk,Bogus",
            "theme": "oled-dark",
        });
        let p = Preferences::from_web_local_storage(&dump).unwrap();
        assert_eq!(p.species.as_deref(), Some("Snow Leopard"));
        assert_eq!(p.partner_gender, vec!["Male", "Female"]);
        assert_eq!(p.partner_species, vec!["any"]);
        assert_eq!(p.kinks, vec!["Biting", "Musk"]);
        assert_eq!(p.language, "any");
        assert!(p.validate().is_ok());

        assert_eq!(Preferences::from_web_local_storage(&json!({"theme": "dark"})), None);
        assert_eq!(Preferences::from_web_local_storage(&json!([1, 2])), None);
    }

    #[test]
    fn summary_is_compact() {
        let mut p = complete();
        assert_eq!(p.summary(Field::Kinks), "Any / All");
        assert_eq!(p.summary(Field::Gender), "Male");
        p.toggle(Field::Kinks, "Anal");
        p.toggle(Field::Kinks, "Biting");
        p.toggle(Field::Kinks, "Musk");
        assert_eq!(p.summary(Field::Kinks), "Anal +2");
        assert_eq!(Preferences::default().summary(Field::Species), "—");
    }

    #[test]
    fn toml_roundtrip() {
        let p = complete();
        let s = toml::to_string(&p).unwrap();
        assert_eq!(toml::from_str::<Preferences>(&s).unwrap(), p);
        // Missing keys fall back to defaults instead of failing to load.
        let partial: Preferences = toml::from_str("gender = \"Male\"").unwrap();
        assert_eq!(partial.kinks, vec!["any"]);
    }
}
