//! Verify by hand as a checklist: the Brief's Verify items, ticked in the session so the ticks
//! outlive the window.

use super::App;

/// The checklist items in a Verify body: its list items, or failing those its paragraphs, with
/// list markers, task boxes and emphasis marks taken off.
pub fn checklist_items(md: &str) -> Vec<String> {
    let clean = |s: &str| s.replace("**", "").replace('`', "").trim().to_owned();
    let marker = |t: &str| -> Option<String> {
        let t = t.trim_start();
        let rest = if let Some(r) = t.strip_prefix(['-', '*', '+']) {
            r
        } else {
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            if digits == 0 || digits > 3 {
                return None;
            }
            t[digits..].strip_prefix(['.', ')'])?
        };
        let rest = rest.strip_prefix(' ')?;
        let rest = ["[ ] ", "[x] ", "[X] "].iter().find_map(|b| rest.strip_prefix(b)).unwrap_or(rest);
        Some(clean(rest))
    };
    let listed: Vec<String> = md.lines().filter_map(marker).filter(|s| !s.is_empty()).collect();
    if !listed.is_empty() {
        return listed;
    }
    md.split("\n\n")
        .map(|p| clean(&p.replace('\n', " ")))
        .filter(|s| !s.is_empty())
        .collect()
}

impl App {
    /// The Brief's Verify by hand items, in order.
    pub fn verify_items(&self) -> Vec<String> {
        self.mirror
            .brief
            .as_ref()
            .and_then(|b| b.verify.as_ref())
            .map(|v| checklist_items(&v.text))
            .unwrap_or_default()
    }

    pub fn verify_done(&self, item: &str) -> bool {
        self.mirror.verified.iter().any(|v| v == item)
    }

    /// `Shift+H` and the card's head: folds the Verify card to its pill, or opens it.
    pub fn toggle_verify_card(&mut self) {
        if self.verify_items().is_empty() {
            self.status = "Nothing to verify by hand: the brief has no Verify items".into();
            return;
        }
        self.brief.verify_shut = !self.brief.verify_shut;
    }

    /// Ticks or unticks a Verify item, in the session.
    pub fn toggle_verified(&mut self, item: &str) {
        let item = item.to_owned();
        self.mutate(|s| {
            match s.verified.iter().position(|v| *v == item) {
                Some(i) => {
                    s.verified.remove(i);
                }
                None => s.verified.push(item),
            }
            s.version += 1;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::checklist_items;

    #[test]
    fn list_items_become_the_checklist() {
        let md = "Run these:\n\n- Open **the** app\n2. Add a `voucher`\n- [x] Check the total\n";
        assert_eq!(checklist_items(md), ["Open the app", "Add a voucher", "Check the total"]);
    }

    #[test]
    fn without_a_list_each_paragraph_is_an_item() {
        assert_eq!(
            checklist_items("Open the app.\n\nAdd a voucher\nthen save."),
            ["Open the app.", "Add a voucher then save."]
        );
    }
}
