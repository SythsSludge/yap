//! The fixed option lists the YiffSpot server validates preferences against.
//!
//! These lists are generated from `vendor/yiffspot/src/models/*.js` and must stay
//! byte-identical to the server's copies: the server rejects any value it does not
//! know with `invalid_preferences`. They are data from YiffSpot, which is
//! MIT-licensed (Copyright (c) 2014 Taylor Locke); see `THIRD_PARTY_NOTICES.md`.

/// The sentinel the server treats as "match anything" for multi-selects and language.
pub const ANY: &str = "any";

/// Longest message the web client will send (it rejects `len >= 3000`).
pub const MAX_MESSAGE_LEN: usize = 3000;

pub const GENDERS: &[&str] = &["Male", "Female", "Gender Fluid", "Intersex", "Transgender", "Non-Binary", "Other"];
pub const ROLES: &[&str] = &["Dominant", "Submissive", "Switch"];
pub const LANGUAGES: &[&str] = &["English", "Spanish", "French", "Deutsch", "Portuguese"];
pub const SPECIES: &[&str] = &[
    "Alligator",
    "Arachnid",
    "Arctic Fox",
    "Badger",
    "Bat",
    "Bear",
    "Bird",
    "Bovine",
    "Cat",
    "Cheetah",
    "Corvid",
    "Cougar",
    "Coyote",
    "Crocodile",
    "Deer",
    "Digimon",
    "Dinosaur",
    "Dog",
    "Dolphin",
    "Donkey",
    "Dragon",
    "Elephant",
    "Ferret",
    "Fish",
    "Fox",
    "Frog",
    "Giraffe",
    "Gryphon",
    "Hedgehog",
    "Horse",
    "Human",
    "Hydra",
    "Hyena",
    "Iguana",
    "Insect",
    "Jackal",
    "Kangaroo",
    "Koala",
    "Leopard",
    "Lion",
    "Lizard",
    "Lynx",
    "Mouse",
    "Newt",
    "Ocelot",
    "Octopus",
    "Other",
    "Otter",
    "Panda",
    "Panther",
    "Pegasus",
    "Pig/Swine",
    "Pokemon",
    "Primate",
    "Protogen",
    "Rabbit",
    "Raccoon",
    "Rat",
    "Red Panda",
    "Salamander",
    "Seal",
    "Sergal",
    "Shark",
    "Sheep",
    "Skunk",
    "Snake",
    "Snow Leopard",
    "Squirrel",
    "Tiger",
    "Turtle",
    "Unicorn",
    "Werewolf",
    "Whale",
    "Wolf",
    "Zebra",
];
pub const KINKS: &[&str] = &[
    "3+ Penetration",
    "Age Differences",
    "Age Progression",
    "Age Regression",
    "All the Way Through",
    "Anal Vore",
    "Anal",
    "Androgyny",
    "Animal Transformation",
    "Aphrodisiacs",
    "Ass to Mouth",
    "Ass Worship",
    "Auto-Fellatio",
    "Barbed Cocks",
    "Begging",
    "Belly Fucking",
    "Bimbofication",
    "Birthing",
    "Biting",
    "Blood",
    "Bloodplay",
    "Body Swapping",
    "Body Writing",
    "Bondage",
    "Branding",
    "Breeding",
    "Bukkake",
    "Caging",
    "Cervical Penetration",
    "Chastity",
    "Choking",
    "Chubby",
    "Clit Play",
    "CNC",
    "Cock Vore",
    "Condoms",
    "Corruption",
    "Creampie",
    "Crossdressing",
    "Crotch Sniffing",
    "Cum Bath",
    "Cum Enemas",
    "Cum",
    "Cuntboys",
    "Deepthroat",
    "Degradation",
    "Diapers",
    "Digestion",
    "Dirty Talking",
    "Docking",
    "Double Penetration",
    "Drug / Alcohol Use",
    "Ear Play",
    "Electric Toys",
    "Enemas",
    "Excessive Cum",
    "Exhibitionism",
    "Exotic Cocks",
    "Face Fucking",
    "Face Sitting",
    "Farting",
    "Felching",
    "Fellatio",
    "Femboys",
    "Femininity",
    "Feminization",
    "Feral",
    "Fisting",
    "Flexibility",
    "Food Play",
    "Foot Play",
    "Foreplay",
    "Foreskin Worship",
    "Frotting",
    "Gags",
    "Gangbangs",
    "Gender Transformation",
    "Glory Hole",
    "Group Sex",
    "Growth",
    "Hair Pulling",
    "Hand Cuffs",
    "Handjob / Fingering",
    "Hard Vore",
    "Horns",
    "Hotdogging",
    "Humiliation",
    "Hyper Asses",
    "Hyper Balls",
    "Hyper Breasts",
    "Hyper Cocks",
    "Hyper Fat",
    "Hyper Muscle",
    "Hyper Vaginas",
    "Hyper",
    "Hyper-Voluptuous",
    "Ice",
    "In Heat",
    "Incest",
    "Inflation",
    "Kissing",
    "Knotted Cocks",
    "Knotting",
    "Lactation",
    "Leash & Collar",
    "Licking",
    "Living Insertions",
    "Macrophilia",
    "Magic Users",
    "Male Pregnancy",
    "Masculinity",
    "Masturbation",
    "Messy",
    "Microphilia",
    "Mind Control",
    "Multiple Orgasms",
    "Musk",
    "Natural Musk",
    "Navel Play",
    "Nipple Penetration",
    "Nipple Piercings",
    "Nursing",
    "Objectification",
    "Objectophilia",
    "Oral Fixation",
    "Orgasm Control",
    "Oviposition",
    "Pain",
    "Paw Play",
    "Pegging",
    "Pet / Master",
    "Piercings",
    "Powerbottoming",
    "Pregnancy",
    "Prolapse",
    "Prostate Play",
    "Prostitution",
    "Public Humiliation",
    "Queefing",
    "Rimming",
    "Rubber / Elastic / Latex",
    "Saliva",
    "Scat",
    "Scissoring",
    "Scratching",
    "Sex Toys",
    "Sexual Frustration",
    "Shaving",
    "Sheath Play",
    "Shrinking",
    "Size Difference",
    "Slapping",
    "Slime",
    "Sloppy Seconds",
    "Small Breasts",
    "Smoking",
    "Snowballing",
    "Soft Vore",
    "Somnophilia",
    "Sounding",
    "Spanking",
    "Squirting",
    "Stomach Bulging",
    "Strap-ons",
    "Strip Tease",
    "Stuckage",
    "Swallowing",
    "Sweat",
    "Tail Pulling",
    "Tail Sex",
    "Tailsex",
    "Teasing",
    "Teeth Play",
    "Tentacles",
    "Throat Penetration",
    "Tickling",
    "Titfucking",
    "Tomboys",
    "Transformation",
    "Twinks",
    "Twins",
    "Udders",
    "Unusual Semen",
    "Urethra Play",
    "Vanilla Sex",
    "Virgin",
    "Vorarephilia",
    "Voyeurism",
    "Watersports",
    "Wax Play",
    "Whipping",
];

/// Which catalog list a preference field draws its options from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Catalog {
    Gender,
    Role,
    Language,
    Species,
    Kinks,
}

impl Catalog {
    pub fn options(self) -> &'static [&'static str] {
        match self {
            Catalog::Gender => GENDERS,
            Catalog::Role => ROLES,
            Catalog::Language => LANGUAGES,
            Catalog::Species => SPECIES,
            Catalog::Kinks => KINKS,
        }
    }

    /// Whether `value` is one the server accepts for this list. `"any"` is not
    /// included here; callers decide whether a field allows it.
    pub fn contains(self, value: &str) -> bool {
        self.options().contains(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn list_sizes_match_upstream() {
        assert_eq!(GENDERS.len(), 7);
        assert_eq!(ROLES.len(), 3);
        assert_eq!(LANGUAGES.len(), 5);
        assert_eq!(SPECIES.len(), 75);
        assert_eq!(KINKS.len(), 191);
    }

    #[test]
    fn lists_have_no_duplicates_or_any_sentinel() {
        for cat in [Catalog::Gender, Catalog::Role, Catalog::Language, Catalog::Species, Catalog::Kinks] {
            let opts = cat.options();
            let unique: HashSet<_> = opts.iter().collect();
            assert_eq!(unique.len(), opts.len(), "{cat:?} has duplicates");
            assert!(!cat.contains(ANY), "{cat:?} must not contain the any sentinel");
        }
    }

    #[test]
    fn partner_kink_lists_can_be_split_on_comma_space() {
        // The server joins kinks with ", " in `partner_connected`; that only
        // round-trips if no kink contains the separator.
        assert!(KINKS.iter().all(|k| !k.contains(", ")));
    }
}
