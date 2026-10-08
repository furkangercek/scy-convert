//! Add a caption: the small window "Add a caption..." opens on GIFs. It asks
//! for the text and where the bar goes, then saves `name-captioned.gif` next
//! to each file and closes; a notification says when it's done.

use std::path::PathBuf;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use scyconvert_core::{Caption, CaptionPlace, Options};

use super::error_text;
use super::theme::{self, Palette, primary_button, secondary_button, text};
use crate::model::{self, AppState, Feedback};

pub struct CaptionView {
    app: Entity<AppState>,
    pub(super) files: Vec<PathBuf>,
    pub(super) text: Entity<InputState>,
    pub(super) place: CaptionPlace,
    pub(super) error: Option<String>,
    _enter: Subscription,
    _appearance: Subscription,
}

impl CaptionView {
    pub fn new(
        app: Entity<AppState>,
        files: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = cx.new(|cx| InputState::new(window, cx).placeholder("Caption text"));
        text.update(cx, |input, cx| input.focus(window, cx));
        let _enter =
            cx.subscribe_in(
                &text,
                window,
                |this: &mut Self, _, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.save(window, cx),
                    InputEvent::Change => {
                        this.error = None;
                        cx.notify();
                    }
                    _ => {}
                },
            );
        Self {
            _appearance: theme::observe_appearance(window, cx),
            app,
            files,
            text,
            place: CaptionPlace::Top,
            error: None,
            _enter,
        }
    }

    fn caption(&self, cx: &App) -> Option<Caption> {
        let text = self.text.read(cx).value().trim().to_string();
        (!text.is_empty()).then_some(Caption {
            text,
            place: self.place,
        })
    }

    /// Queues the captioned copies and closes the window.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(caption) = self.caption(cx) else {
            return;
        };
        let options = Options {
            caption: Some(caption),
            ..Options::default()
        };
        if let Err(e) = options.validate() {
            self.error = Some(e.to_string());
            cx.notify();
            return;
        }
        let files = self.files.clone();
        let queued = self.app.update(cx, |s, cx| {
            s.queue_action_with(
                &files,
                scyconvert_core::actions::CAPTION,
                &options,
                None,
                Feedback::Notify,
                cx,
            )
        });
        match queued {
            Ok(_) => window.remove_window(),
            Err(e) => {
                self.error = Some(e);
                cx.notify();
            }
        }
    }

    fn header(&self, p: &Palette) -> Div {
        let what = match self.files.as_slice() {
            [one] => model::file_name(one).to_string(),
            many => format!("{} GIFs", many.len()),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                text(15., 20., p.text)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Add a caption"),
            )
            .child(
                text(12., 16., p.secondary)
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(what),
            )
    }
}

impl Render for CaptionView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let empty = self.caption(cx).is_none();
        let places = [
            ("top", SharedString::from("Top")),
            ("bottom", SharedString::from("Bottom")),
        ];
        let selected = self.place.id();
        let view = cx.entity().downgrade();
        let place = theme::segmented("place", &places, selected, &p, move |key, _, cx| {
            let _ = view.update(cx, |this, cx| {
                this.place = if key == "bottom" {
                    CaptionPlace::Bottom
                } else {
                    CaptionPlace::Top
                };
                cx.notify();
            });
        });
        div()
            .id("caption")
            .flex()
            .flex_col()
            .size_full()
            .bg(p.window)
            .font_family(theme::SANS)
            .text_color(p.text)
            .children(theme::title_bar(
                Some("Add a caption"),
                44.,
                p.dark.then_some(p.chrome),
                &p,
            ))
            .when(!theme::transparent_titlebar(), |d| d.pt(px(20.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap(px(14.))
                    .px(px(24.))
                    .child(self.header(&p))
                    .child(theme::field(&self.text, "caption-text"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(text(12., 16., p.secondary).child("Bar"))
                            .child(place),
                    )
                    .child(
                        text(12., 16., p.tertiary)
                            .child("Black bold text on a white bar. Long captions wrap."),
                    )
                    .children(self.error.clone().map(|e| error_text(e, &p))),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_end()
                    .gap(px(10.))
                    .px(px(24.))
                    .py(px(14.))
                    .bg(p.recessed)
                    .border_t_1()
                    .border_color(p.hairline)
                    .child(
                        secondary_button("cancel", "Cancel", &p)
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        primary_button("save", "Save GIF", 13., empty).when(!empty, |b| {
                            b.on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))
                        }),
                    ),
            )
    }
}
