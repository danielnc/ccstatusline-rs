//! herdr-workspace / herdr-tab / herdr-pane (not upstream): the label of the
//! herdr workspace, tab or pane this session runs in, falling back to its id.
//! Outside herdr they render nothing. See `crate::misc::herdr`.

use crate::context::RenderContext;
use crate::misc::herdr::{lookup_process, Kind, Target};
use crate::settings::WidgetItem;
use crate::widgets::shared::raw_or_labeled;
use crate::widgets::Widget;

pub struct Herdr {
    kind: Kind,
    label_prefix: &'static str,
    preview: &'static str,
}

pub static HERDR_WORKSPACE: Herdr =
    Herdr { kind: Kind::Workspace, label_prefix: "Workspace: ", preview: "my-workspace" };
pub static HERDR_TAB: Herdr = Herdr { kind: Kind::Tab, label_prefix: "Tab: ", preview: "my-tab" };
pub static HERDR_PANE: Herdr = Herdr { kind: Kind::Pane, label_prefix: "Pane: ", preview: "my-pane" };

fn display(t: &Target) -> &str {
    t.label.as_deref().unwrap_or(&t.id)
}

impl Herdr {
    fn render_target(&self, item: &WidgetItem, is_preview: bool, target: Option<Target>) -> Option<String> {
        if is_preview {
            return Some(raw_or_labeled(item, self.label_prefix, self.preview));
        }
        let t = target?;
        Some(raw_or_labeled(item, self.label_prefix, display(&t)))
    }
}

impl Widget for Herdr {
    fn default_color(&self) -> &'static str {
        "magenta"
    }
    fn display_name(&self) -> &'static str {
        match self.kind {
            Kind::Workspace => "Herdr Workspace",
            Kind::Tab => "Herdr Tab",
            Kind::Pane => "Herdr Pane",
        }
    }
    fn description(&self) -> &'static str {
        match self.kind {
            Kind::Workspace => "Shows the label of the herdr workspace this session runs in",
            Kind::Tab => "Shows the label of the herdr tab this session runs in",
            Kind::Pane => "Shows the label of the herdr pane this session runs in (its id when unnamed)",
        }
    }
    fn category(&self) -> &'static str {
        "Environment"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return self.render_target(item, true, None);
        }
        self.render_target(item, false, lookup_process(self.kind))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use super::*;
    use crate::misc::herdr::lookup;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn renders_nothing_outside_herdr() {
        let t = lookup(Kind::Tab, &env_of(&[("HERDR_TAB_ID", "w1:t1")]));
        assert_eq!(HERDR_TAB.render_target(&item("herdr-tab"), false, t), None);
    }

    #[test]
    fn falls_back_to_id_and_respects_raw_and_label() {
        let env = env_of(&[("HERDR_ENV", "1"), ("HERDR_PANE_ID", "w1:p1")]);
        let t = lookup(Kind::Pane, &env);
        assert_eq!(HERDR_PANE.render_target(&item("herdr-pane"), false, t.clone()).as_deref(), Some("Pane: w1:p1"));
        let mut raw = item("herdr-pane");
        raw.raw_value = Some(true);
        assert_eq!(HERDR_PANE.render_target(&raw, false, t.clone()).as_deref(), Some("w1:p1"));
        let labeled = with_meta(item("herdr-pane"), &[("label", "▣ ")]);
        assert_eq!(HERDR_PANE.render_target(&labeled, false, t).as_deref(), Some("▣ w1:p1"));
    }

    #[test]
    fn prefers_live_label() {
        let t = Target { id: "w1:t1".into(), label: Some("ccstatusline-rs".into()) };
        assert_eq!(
            HERDR_TAB.render_target(&item("herdr-tab"), false, Some(t)).as_deref(),
            Some("Tab: ccstatusline-rs")
        );
        assert_eq!(HERDR_TAB.render_target(&item("herdr-tab"), true, None).as_deref(), Some("Tab: my-tab"));
    }
}
