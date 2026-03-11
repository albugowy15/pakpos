use iced::{
    Font, Renderer, Theme, font,
    widget::{Text, text},
};

pub fn bold(header: &str) -> Text<'_, Theme, Renderer> {
    text(header).font(Font {
        weight: font::Weight::Bold,
        ..Font::DEFAULT
    })
}
