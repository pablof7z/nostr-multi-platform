use egui::{Align, Color32, Layout, TextEdit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingZap {
    pub(crate) recipient_pubkey: String,
    pub(crate) target_event_id: String,
}

impl PendingZap {
    pub(crate) fn new(
        recipient_pubkey: impl Into<String>,
        target_event_id: impl Into<String>,
    ) -> Self {
        Self {
            recipient_pubkey: recipient_pubkey.into(),
            target_event_id: target_event_id.into(),
        }
    }
}

pub(crate) const DEFAULT_ZAP_SATS: u64 = 21;
pub(crate) const ZAP_PRESET_SATS: [u64; 6] = [21, 100, 500, 1_000, 5_000, 21_000];

#[derive(Debug)]
pub(crate) struct ZapAmountState {
    pending: Option<PendingZap>,
    amount_sats: String,
}

impl Default for ZapAmountState {
    fn default() -> Self {
        Self {
            pending: None,
            amount_sats: DEFAULT_ZAP_SATS.to_string(),
        }
    }
}

impl ZapAmountState {
    pub(crate) fn open(&mut self, target: PendingZap) {
        self.pending = Some(target);
        self.amount_sats = DEFAULT_ZAP_SATS.to_string();
    }

    fn close(&mut self) {
        self.pending = None;
        self.amount_sats = DEFAULT_ZAP_SATS.to_string();
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Option<(PendingZap, u64)> {
        let target = self.pending.clone()?;
        let mut open = true;
        let mut close = false;
        let mut send_amount_msats = None;

        egui::Window::new("Send Zap")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Amount");
                ui.horizontal_wrapped(|ui| {
                    for sats in ZAP_PRESET_SATS {
                        if ui.small_button(zap_preset_label(sats)).clicked() {
                            self.amount_sats = sats.to_string();
                        }
                    }
                });
                ui.add(
                    TextEdit::singleline(&mut self.amount_sats)
                        .hint_text("sats")
                        .desired_width(160.0),
                );

                let amount_msats = parse_zap_msats(&self.amount_sats);
                if amount_msats.is_none() {
                    ui.colored_label(
                        Color32::from_rgb(248, 113, 113),
                        "Enter a positive sat amount",
                    );
                }

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(amount_msats.is_some(), egui::Button::new("Zap"))
                            .clicked()
                        {
                            send_amount_msats = amount_msats;
                        }
                    });
                });
            });

        if let Some(amount_msats) = send_amount_msats {
            self.close();
            Some((target, amount_msats))
        } else if !open || close {
            self.close();
            None
        } else {
            None
        }
    }
}

pub(crate) fn zap_msats_from_sats(sats: u64) -> Option<u64> {
    sats.checked_mul(1_000).filter(|msats| *msats > 0)
}

pub(crate) fn parse_zap_msats(raw: &str) -> Option<u64> {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    let sats = digits.parse::<u64>().ok()?;
    zap_msats_from_sats(sats)
}

pub(crate) fn zap_preset_label(sats: u64) -> String {
    format!("{sats} sats")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_sats_to_millisats() {
        assert_eq!(zap_msats_from_sats(21), Some(21_000));
        assert_eq!(zap_msats_from_sats(21_000), Some(21_000_000));
    }

    #[test]
    fn parses_digit_only_custom_amounts() {
        assert_eq!(parse_zap_msats("100 sats"), Some(100_000));
        assert_eq!(parse_zap_msats("1,000"), Some(1_000_000));
    }

    #[test]
    fn rejects_empty_zero_and_overflow() {
        assert_eq!(parse_zap_msats(""), None);
        assert_eq!(parse_zap_msats("0"), None);
        assert_eq!(zap_msats_from_sats(u64::MAX), None);
    }
}
