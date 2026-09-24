//! The media viewer: a full-window photo and video view with the chat's
//! album along the bottom, in the shape WhatsApp's own viewer uses.

use egui::{Align, CornerRadius, Layout, Rect, Sense, Stroke, UiBuilder, Vec2, pos2, vec2};

use std::path::Path;

use crate::app::App;
use crate::archive::ChatMedia;
use crate::model::{Action, Dialog};
use crate::theme::{self, Icon};

use super::conversation::thumbnail_uri;

/// Height of the header across the top.
const HEADER: f32 = 56.0;
/// Height of the album strip along the bottom.
const STRIP: f32 = 78.0;
const THUMB: f32 = 56.0;
const STRIP_GAP: f32 = 6.0;
const STRIP_PAD: f32 = 8.0;

/// WhatsApp paints its viewer over the whole window in near black, so the
/// picture is the only thing with colour in it.
const BACKDROP: egui::Color32 = egui::Color32::from_rgb(0x0b, 0x0e, 0x11);

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(preview) = app.image_preview.clone() else {
        return;
    };
    let palette = app.palette;
    // The album the viewer browses, and where this picture sits in it.
    let album = app.viewer_media.clone();
    let position = preview.position(&album);
    let item = position.and_then(|index| album.get(index)).cloned();
    let message = preview.message().to_owned();
    let chat = preview.chat().to_owned();
    let strip = album.len() > 1;
    let mut actions: Vec<Action> = Vec::new();

    let mut backdrop_clicked = false;
    // The viewer fills the window instead of floating a dialog over the chat:
    // the header across the top, the picture on the black field, and the album
    // along the bottom, which is the shape WhatsApp's own viewer uses. It stays
    // a modal, so it keeps the keyboard: Tab walks its own controls and Enter
    // cannot reach the chat behind it.
    let screen = ctx.content_rect();
    let area = egui::Area::new(egui::Id::new("image-preview"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min);
    let response = egui::Modal::new(egui::Id::new("image-preview"))
        .area(area)
        .frame(egui::Frame::new().fill(BACKDROP))
        .backdrop_color(BACKDROP)
        .show(ctx, |ui| {
            // Sized to the window with no margin around it, so the field runs
            // edge to edge.
            ui.set_min_size(screen.size());
            let window = Rect::from_min_size(screen.min, screen.size());
            ui.painter().rect_filled(window, 0.0, BACKDROP);
            // Claimed first, so every control drawn over it wins the click.
            // What reaches this is a click on the empty field, which closes the
            // viewer, as it does on the phone.
            let (_, backdrop) = ui.allocate_exact_size(window.size(), Sense::click());
            let (header_rect, stage, strip_rect) = panes(window, strip);

            let mut head = ui.new_child(UiBuilder::new().max_rect(header_rect));
            head.set_clip_rect(header_rect);
            header(
                app,
                &mut head,
                &preview,
                &chat,
                item.is_some(),
                &mut actions,
            );

            let mut body = ui.new_child(UiBuilder::new().max_rect(stage));
            body.set_clip_rect(stage);
            match item.as_ref().filter(|item| item.video) {
                Some(item) => video(app, &mut body, item, stage, &chat, &message, &mut actions),
                None => still(
                    app,
                    &mut body,
                    &preview,
                    item.as_ref(),
                    &chat,
                    stage,
                    &mut actions,
                ),
            }
            if strip {
                chevrons(app, ui, &palette, stage, &mut actions);
                let mut bar = ui.new_child(UiBuilder::new().max_rect(strip_rect));
                bar.set_clip_rect(strip_rect);
                strip_bar(
                    app,
                    &mut bar,
                    &palette,
                    &album,
                    &message,
                    strip_rect,
                    &mut actions,
                );
            }
            backdrop_clicked = backdrop.clicked();
        });
    if response.should_close() || backdrop_clicked {
        actions.push(Action::CloseImagePreview);
    }
    app.actions.extend(actions);
}

/// Header, stage and album strip across the window, top to bottom. Without an
/// album the strip is empty and the stage runs to the bottom edge.
fn panes(window: Rect, strip: bool) -> (Rect, Rect, Rect) {
    let header = Rect::from_min_max(window.min, pos2(window.max.x, window.min.y + HEADER));
    let strip_rect = if strip {
        Rect::from_min_max(pos2(window.min.x, window.max.y - STRIP), window.max)
    } else {
        Rect::from_min_max(pos2(window.max.x, window.max.y), window.max)
    };
    let stage = Rect::from_min_max(
        pos2(window.min.x, header.max.y),
        pos2(window.max.x, strip_rect.min.y),
    );
    (header, stage, strip_rect)
}

/// Title, the actions that reach the message, and the zoom controls.
fn header(
    app: &mut App,
    ui: &mut egui::Ui,
    preview: &crate::image_preview::PreviewState,
    chat: &str,
    has_message: bool,
    actions: &mut Vec<Action>,
) {
    let palette = app.palette;
    // The chat's name is the useful title once the viewer browses an album;
    // the file name is all there is for a picture opened on its own.
    let title = app
        .chat(chat)
        .map(|known| app.chat_title(known))
        .unwrap_or_else(|| {
            preview
                .path()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map_or_else(
                    || crate::i18n::gettext(app.locale, "Image").into_owned(),
                    str::to_owned,
                )
        });
    let message = preview.message().to_owned();
    ui.horizontal(|ui| {
        crate::ui::widgets::rich_text(ui, &title, theme::semibold(14.0), palette.text);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::icon_button(
                ui,
                Icon::X,
                18.0,
                palette.secondary,
                palette.text,
                crate::i18n::gettext(app.locale, "Close preview (Esc)").as_ref(),
            )
            .clicked()
            {
                actions.push(Action::CloseImagePreview);
            }
            if let Some(path) = preview.path()
                && theme::icon_button(
                    ui,
                    Icon::ExternalLink,
                    18.0,
                    palette.secondary,
                    palette.text,
                    crate::i18n::gettext(app.locale, "Open in another app").as_ref(),
                )
                .clicked()
            {
                actions.push(Action::OpenFile(path.to_owned()));
            }
            ui.add_space(8.0);
            // Right to left: zoom in, the current scale, zoom out.
            if theme::icon_button(
                ui,
                Icon::Plus,
                18.0,
                palette.secondary,
                palette.text,
                crate::i18n::gettext(app.locale, "Zoom in").as_ref(),
            )
            .clicked()
            {
                actions.push(Action::ZoomImageIn);
            }
            // One control shows the scale and switches between fitting the
            // window and the original size.
            let (label, hint, action) = if preview.is_fit() {
                (
                    crate::i18n::gettext(app.locale, "Fit").into_owned(),
                    crate::i18n::gettext(app.locale, "Show at original size"),
                    Action::ImageActualSize,
                )
            } else {
                (
                    format!("{:.0}%", preview.zoom() * 100.0),
                    crate::i18n::gettext(app.locale, "Fit to the window (0)"),
                    Action::FitImage,
                )
            };
            if theme::soft_button(ui, &palette, None, &label, false)
                .on_hover_text(hint)
                .clicked()
            {
                actions.push(action);
            }
            if theme::icon_button(
                ui,
                Icon::Minus,
                18.0,
                palette.secondary,
                palette.text,
                crate::i18n::gettext(app.locale, "Zoom out").as_ref(),
            )
            .clicked()
            {
                actions.push(Action::ZoomImageOut);
            }
            // What the message itself can do, when the viewer knows which one
            // it is. A picture opened on its own has nothing to act on.
            if has_message {
                ui.add_space(8.0);
                if theme::icon_button(
                    ui,
                    Icon::Reply,
                    18.0,
                    palette.secondary,
                    palette.text,
                    crate::i18n::gettext(app.locale, "Reply").as_ref(),
                )
                .clicked()
                {
                    actions.push(Action::CloseImagePreview);
                    actions.push(Action::Reply(message.clone()));
                }
                if theme::icon_button(
                    ui,
                    Icon::Smile,
                    18.0,
                    palette.secondary,
                    palette.text,
                    crate::i18n::gettext(app.locale, "React").as_ref(),
                )
                .clicked()
                {
                    // The viewer draws after the picker, so it has to go first:
                    // otherwise the picker opens behind it and cannot be used.
                    actions.push(Action::CloseImagePreview);
                    actions.push(Action::OpenReactionPicker {
                        chat: chat.to_owned(),
                        message: message.clone(),
                        beside_menu: false,
                    });
                }
                if theme::icon_button(
                    ui,
                    Icon::Forward,
                    18.0,
                    palette.secondary,
                    palette.text,
                    crate::i18n::gettext(app.locale, "Forward").as_ref(),
                )
                .clicked()
                {
                    actions.push(Action::CloseImagePreview);
                    actions.push(Action::ShowDialog(Dialog::Forward {
                        chat: chat.to_owned(),
                        messages: vec![message.clone()],
                    }));
                }
                if theme::icon_button(
                    ui,
                    Icon::Download,
                    18.0,
                    palette.secondary,
                    palette.text,
                    crate::i18n::gettext(app.locale, "Download").as_ref(),
                )
                .clicked()
                {
                    actions.push(Action::Download {
                        card: None,
                        chat: chat.to_owned(),
                        message: message.clone(),
                    });
                }
                if theme::icon_button(
                    ui,
                    Icon::MessageCircle,
                    18.0,
                    palette.secondary,
                    palette.text,
                    crate::i18n::gettext(app.locale, "Show in the chat").as_ref(),
                )
                .clicked()
                {
                    actions.push(Action::CloseImagePreview);
                    actions.push(Action::OpenMessage {
                        chat: chat.to_owned(),
                        message: message.clone(),
                    });
                }
            }
        });
    });
}

/// The picture itself, zoomed and panned exactly as before.
fn still(
    app: &mut App,
    ui: &mut egui::Ui,
    preview: &crate::image_preview::PreviewState,
    item: Option<&ChatMedia>,
    chat: &str,
    stage: Rect,
    actions: &mut Vec<Action>,
) {
    let palette = app.palette;
    let canvas = stage.size();
    // An item whose attachment is not here yet is shown for what it is, with
    // its thumbnail and a way to fetch it, instead of standing in for another
    // picture.
    let Some(path) = preview.path() else {
        pending(app, ui, item, preview.message(), chat, stage, actions);
        return;
    };
    // Registered with the image cache like every other draw site, so a sweep
    // never releases the picture while it is on screen.
    let image = crate::ui::widgets::file_image(ui, path);
    match image.load_for_size(ui.ctx(), canvas) {
        Ok(egui::load::TexturePoll::Ready { texture }) => {
            let size = display_size(texture.size, canvas, preview.is_fit(), preview.zoom());
            if preview.is_fit()
                && texture.size.x > 0.0
                && let Some(state) = &mut app.image_preview
            {
                state.set_fit_scale(size.x / texture.size.x);
            }
            egui::ScrollArea::both()
                .id_salt("image-preview-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.allocate_ui_with_layout(
                        canvas.max(size),
                        Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.add(image.fit_to_exact_size(size));
                        },
                    );
                });
        }
        Ok(egui::load::TexturePoll::Pending { .. }) => {
            theme::paint_spinner(ui, stage, 28.0, palette.accent);
        }
        Err(_) => {
            let path = path.to_owned();
            ui.allocate_ui_with_layout(
                canvas,
                Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label(crate::i18n::gettext(
                        app.locale,
                        "This image could not be displayed in ZapFast.",
                    ));
                    if ui
                        .button(crate::i18n::gettext(app.locale, "Open externally"))
                        .clicked()
                    {
                        app.actions.push(Action::OpenFile(path.clone()));
                    }
                },
            );
        }
    }
}

/// An item whose attachment has not been downloaded: its own thumbnail, the
/// clip or picture it is, and the button that fetches it. Nothing here stands
/// in for another file.
fn pending(
    app: &App,
    ui: &mut egui::Ui,
    item: Option<&ChatMedia>,
    message: &str,
    chat: &str,
    stage: Rect,
    actions: &mut Vec<Action>,
) {
    let disc = Rect::from_center_size(stage.center(), Vec2::splat(220.0));
    if let Some(bytes) = item.and_then(|item| item.thumbnail.as_deref()) {
        egui::Image::new(thumbnail_uri(ui.ctx(), chat, message, bytes))
            .fit_to_exact_size(disc.size())
            .corner_radius(8.0)
            .paint_at(ui, disc);
    } else {
        theme::paint_icon(
            ui,
            if item.is_some_and(|item| item.video) {
                Icon::Video
            } else {
                Icon::Image
            },
            disc,
            48.0,
            egui::Color32::from_white_alpha(150),
        );
    }
    let button = Rect::from_center_size(
        pos2(stage.center().x, disc.bottom() + 34.0),
        vec2(160.0, 32.0),
    );
    let response = ui
        .interact(
            button,
            egui::Id::new("viewer-pending-download"),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.painter()
        .rect_filled(button, 6.0, egui::Color32::from_white_alpha(28));
    ui.painter().text(
        button.center(),
        egui::Align2::CENTER_CENTER,
        crate::i18n::gettext(app.locale, "Download"),
        theme::regular(13.5),
        egui::Color32::WHITE,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            crate::i18n::gettext(app.locale, "Download").as_ref(),
        )
    });
    theme::reveal_focus(&response);
    if response.clicked() {
        actions.push(Action::Download {
            card: None,
            chat: chat.to_owned(),
            message: message.to_owned(),
        });
    }
}

/// A video plays in place, through the same player the chat bubble uses, so it
/// carries sound and its position is the one the player reports.
fn video(
    app: &mut App,
    ui: &mut egui::Ui,
    item: &ChatMedia,
    stage: Rect,
    chat: &str,
    message: &str,
    actions: &mut Vec<Action>,
) {
    let palette = app.palette;
    // A clip keeps its own shape: the decoded frame knows it, and 16:9 is only
    // what an undecoded poster falls back to.
    let status = app.video.status(message);
    let aspect = status
        .as_ref()
        .and_then(|status| status.frame.as_ref())
        .map(|frame| frame.size())
        .filter(|size| size[0] > 0 && size[1] > 0)
        .map_or_else(
            || vec2(16.0, 9.0),
            |size| vec2(size[0] as f32, size[1] as f32),
        );
    let size = fit(aspect, stage.size());
    let media = Rect::from_center_size(stage.center(), size);
    // The player pauses a clip it has not been told about for 1.5 s, so the
    // viewer has to say it is still on screen, as the chat renderer does.
    app.video.saw(message);
    match status.as_ref().and_then(|status| status.frame.clone()) {
        Some(frame) => {
            ui.painter().image(
                frame.id(),
                media,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            ui.painter()
                .rect_filled(media, 8.0, egui::Color32::from_black_alpha(120));
            if let Some(bytes) = item.thumbnail.as_deref() {
                egui::Image::new(thumbnail_uri(ui.ctx(), chat, message, bytes))
                    .fit_to_exact_size(media.size())
                    .corner_radius(8.0)
                    .paint_at(ui, media);
            }
        }
    }
    // Clicking the frame plays or pauses, as in the bubble. A clip that is not
    // here yet is offered for download instead.
    let response = ui
        .interact(media, egui::Id::new("viewer-video"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let label = if item.path.as_deref().is_some_and(Path::is_file) {
        crate::i18n::gettext(app.locale, "Play or pause the video")
    } else {
        crate::i18n::gettext(app.locale, "Download")
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.as_ref())
    });
    theme::reveal_focus(&response);
    if response.clicked() {
        match item.path.clone().filter(|path| path.is_file()) {
            Some(path) => actions.push(Action::PlayVideo {
                message: message.to_owned(),
                path,
            }),
            None => actions.push(Action::Download {
                card: None,
                chat: chat.to_owned(),
                message: message.to_owned(),
            }),
        }
    }
    let playing = status
        .as_ref()
        .is_some_and(|status| status.state == crate::video::State::Playing);
    if playing {
        // A thin line along the bottom of the frame, so a long clip has a
        // position. It sits inside the frame: the body is clipped to the
        // stage, and a wide window makes the frame as tall as the stage, so a
        // bar below it would be painted outside the clip and never seen.
        let bar = Rect::from_min_size(
            pos2(media.left(), media.bottom() - 7.0),
            vec2(media.width(), 3.0),
        );
        ui.painter()
            .rect_filled(bar, 1.5, egui::Color32::from_white_alpha(40));
        let fraction = status.map_or(0.0, |status| status.fraction());
        let played = Rect::from_min_size(bar.min, vec2(bar.width() * fraction, bar.height()));
        ui.painter().rect_filled(played, 1.5, palette.accent);
    } else {
        let disc = Rect::from_center_size(media.center(), Vec2::splat(64.0));
        ui.painter()
            .circle_filled(disc.center(), 32.0, egui::Color32::from_black_alpha(140));
        theme::paint_icon(ui, Icon::Play, disc, 28.0, egui::Color32::WHITE);
    }
}

/// The album along the bottom: a thumbnail per item, the current one ringed.
fn strip_bar(
    app: &App,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    album: &[ChatMedia],
    current: &str,
    rect: Rect,
    actions: &mut Vec<Action>,
) {
    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(120));
    let inner = rect.shrink2(vec2(STRIP_PAD, STRIP_PAD));
    let pad = ((inner.width() - THUMB) * 0.5).max(0.0);
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
    // The app style floats scrollbars over content. This strip needs the bar
    // under the thumbs, with a handle that is not the same colour as its rail.
    {
        let scroll = &mut child.spacing_mut().scroll;
        scroll.floating = false;
        scroll.bar_width = 8.0;
        scroll.bar_inner_margin = 4.0;
        scroll.bar_outer_margin = 2.0;
        scroll.foreground_color = true;
    }
    egui::ScrollArea::horizontal()
        .id_salt("viewer-strip")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.set_min_height(THUMB);
                ui.add_space(pad);
                for (index, item) in album.iter().enumerate() {
                    if index > 0 {
                        ui.add_space(STRIP_GAP);
                    }
                    let (thumb_rect, response) =
                        ui.allocate_exact_size(Vec2::splat(THUMB), Sense::click());
                    if item.id == current {
                        // Keep the picture being looked at in the middle of the
                        // strip as the album is stepped through.
                        ui.scroll_to_rect_animation(
                            thumb_rect,
                            Some(egui::Align::Center),
                            egui::style::ScrollAnimation::none(),
                        );
                    }
                    if ui.is_rect_visible(thumb_rect) {
                        thumb(ui, palette, item, thumb_rect, item.id == current);
                    }
                    // Custom-painted and clickable, so it needs the same focus
                    // reveal and label every other custom control registers.
                    let label = format!(
                        "{} {}",
                        crate::i18n::gettext(
                            app.locale,
                            if item.video { "Video" } else { "Photo" },
                        ),
                        index + 1
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            item.id == current,
                            &label,
                        )
                    });
                    theme::reveal_focus(&response);
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        actions.push(Action::ViewImage {
                            message: item.id.clone(),
                        });
                    }
                }
                ui.add_space(pad);
            });
        });
}

fn thumb(
    ui: &egui::Ui,
    palette: &crate::theme::Palette,
    item: &ChatMedia,
    rect: Rect,
    current: bool,
) {
    ui.painter()
        .rect_filled(rect, 6.0, egui::Color32::from_white_alpha(16));
    if let Some(path) = item.path.as_deref().filter(|path| path.is_file()) {
        egui::Image::new(crate::util::image_uri(path))
            .fit_to_exact_size(rect.size())
            .corner_radius(6.0)
            .paint_at(ui, rect);
    } else if let Some(bytes) = item.thumbnail.as_deref() {
        egui::Image::new(thumbnail_uri(ui.ctx(), &item.id, &item.id, bytes))
            .fit_to_exact_size(rect.size())
            .corner_radius(6.0)
            .paint_at(ui, rect);
    } else {
        theme::paint_icon(
            ui,
            if item.video { Icon::Video } else { Icon::Image },
            rect,
            22.0,
            egui::Color32::from_white_alpha(180),
        );
    }
    if item.video {
        let disc = Rect::from_center_size(rect.center(), Vec2::splat(18.0));
        ui.painter()
            .circle_filled(disc.center(), 9.0, egui::Color32::from_black_alpha(140));
        theme::paint_icon(ui, Icon::Play, disc, 12.0, egui::Color32::WHITE);
    }
    if current {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(2.0, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
}

fn chevrons(
    app: &App,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    stage: Rect,
    actions: &mut Vec<Action>,
) {
    for (left, icon, step, hint) in [
        (
            true,
            Icon::ChevronLeft,
            -1i8,
            crate::i18n::gettext(app.locale, "Previous (Left)").as_ref(),
        ),
        (
            false,
            Icon::ChevronRight,
            1i8,
            crate::i18n::gettext(app.locale, "Next (Right)").as_ref(),
        ),
    ] {
        let x = if left {
            stage.left() + 28.0
        } else {
            stage.right() - 28.0
        };
        let hit = Rect::from_center_size(pos2(x, stage.center().y), Vec2::splat(44.0));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(hit)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        if theme::circle_button(
            &mut child,
            icon,
            36.0,
            egui::Color32::from_black_alpha(120),
            palette.surface_hover,
            egui::Color32::WHITE,
            hint,
        )
        .clicked()
        {
            actions.push(Action::ViewerStep(step));
        }
    }
}

/// Size the image is drawn at from the texture's intrinsic pixel dimensions:
/// fitted into the canvas, or scaled by the preview's zoom factor. Zoom is
/// applied here only. The size hint passed when loading does not change the
/// texture: egui decodes raster formats (all the preview accepts) once at full
/// resolution and reports the source size, whatever size is asked for.
fn display_size(original: Vec2, canvas: Vec2, fit: bool, zoom: f32) -> Vec2 {
    let (width, height) = if fit {
        crate::image_preview::fit_size(original.x, original.y, canvas.x, canvas.y)
    } else {
        crate::image_preview::zoomed_size(original.x, original.y, zoom)
    };
    vec2(width, height)
}

/// Scales `size` to fit inside `max`, keeping its aspect ratio.
fn fit(size: Vec2, max: Vec2) -> Vec2 {
    if size.x <= 0.0 || size.y <= 0.0 {
        return max;
    }
    let scale = (max.x / size.x).min(max.y / size.y);
    size * scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_images_keep_aspect_ratio_inside_the_canvas() {
        assert_eq!(
            display_size(vec2(1600.0, 1200.0), vec2(800.0, 700.0), true, 1.0),
            vec2(800.0, 600.0)
        );
        assert_eq!(
            display_size(vec2(320.0, 240.0), vec2(800.0, 700.0), true, 1.0),
            vec2(320.0, 240.0)
        );
        assert_eq!(
            display_size(vec2(320.0, 240.0), vec2(800.0, 700.0), false, 2.0),
            vec2(640.0, 480.0)
        );
    }

    #[test]
    fn the_panes_fill_the_window_top_to_bottom() {
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0));
        let (header, stage, strip) = panes(window, true);
        assert_eq!(header.min, window.min, "the header starts at the top edge");
        assert_eq!(header.height(), HEADER);
        assert_eq!(strip.max, window.max, "the strip ends at the bottom edge");
        assert_eq!(strip.height(), STRIP);
        assert_eq!(stage.min.y, header.max.y, "no gap under the header");
        assert_eq!(stage.max.y, strip.min.y, "no gap over the strip");
        assert_eq!(stage.width(), window.width(), "the stage is full width");
        // Without an album the stage keeps the room the strip would have taken.
        let (_, stage, strip) = panes(window, false);
        assert!(strip.height() < f32::EPSILON);
        assert_eq!(stage.max.y, window.max.y);
    }

    #[test]
    fn a_video_frame_keeps_its_aspect_ratio_inside_the_stage() {
        assert_eq!(
            fit(vec2(1920.0, 1080.0), vec2(800.0, 600.0)),
            vec2(800.0, 450.0)
        );
        assert_eq!(fit(vec2(0.0, 0.0), vec2(800.0, 600.0)), vec2(800.0, 600.0));
    }
}
