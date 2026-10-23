//! link (upstream `src/widgets/Link.tsx`).

use super::widget_info;
use crate::context::RenderContext;
use crate::jsnum::js_trim;
use crate::settings::WidgetItem;
use crate::widgets::shared::osc8_link;
use crate::widgets::Widget;

pub struct Link;

pub static LINK: Link = Link;

impl Widget for Link {
    widget_info!("cyan", "Link", "Displays a clickable terminal hyperlink using OSC 8", "Custom");
    fn render(&self, item: &WidgetItem, _ctx: &RenderContext) -> Option<String> {
        let url = item.meta("url").map(js_trim).unwrap_or("");
        let text = item.meta("text").map(js_trim).filter(|t| !t.is_empty());
        let label = text.unwrap_or(if url.is_empty() { "no url" } else { url });
        let display = if item.raw() { label.to_string() } else { format!("🔗 {label}") };
        if url.is_empty() {
            return Some(display);
        }
        let Some(href) = crate::misc::url::parse_http_url(url) else { return Some(display) };
        // the parsed form percent-encodes what a link can't carry as is
        let target = if crate::sanitize::is_safe_hyperlink_url(url) { url.to_string() } else { href };
        Some(osc8_link(&target, &display))
    }
}
