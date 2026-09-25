//! The partner's kinks set against yours, with what each one means.

use super::*;

/// Kinks sorted for comparing with a partner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KinkGroups {
    /// On both lists.
    pub shared: Vec<String>,
    /// Only on the partner's list.
    pub theirs: Vec<String>,
    /// Only on yours.
    pub mine: Vec<String>,
}

impl KinkGroups {
    /// `theirs` against `mine`, keeping each list's order. "Any" never counts as shared.
    pub fn compare(theirs: &[&str], mine: &[String]) -> Self {
        let (shared, theirs): (Vec<String>, Vec<String>) =
            theirs.iter().map(|k| (*k).to_owned()).partition(|k| k != ANY && mine.iter().any(|m| m == k));
        let mine = mine.iter().filter(|m| *m != ANY && !shared.contains(m)).cloned().collect();
        KinkGroups { shared, theirs, mine }
    }
}

impl App {
    /// The current partner's kinks against the active profile's, if there's a partner.
    pub fn kink_groups(&self) -> Option<KinkGroups> {
        let PartnerState::Connected(info) = &self.partner else { return None };
        Some(KinkGroups::compare(&info.kink_list(), &self.config.active().preferences.kinks))
    }

    pub fn open_kinks(&mut self) {
        self.modal = Some(Modal::Kinks { scroll: 0 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_keep_order_and_ignore_any() {
        let mine = vec!["Musk".to_owned(), "Kissing".to_owned(), ANY.to_owned()];
        let g = KinkGroups::compare(&["Biting", "Musk", ANY], &mine);
        assert_eq!(g.shared, ["Musk"]);
        assert_eq!(g.theirs, ["Biting", ANY]);
        assert_eq!(g.mine, ["Kissing"]);
    }
}
