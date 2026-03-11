use iced::{
    Background, Theme, border,
    theme::palette,
    widget::button::{Status, Style},
};

pub fn background(theme: &Theme, status: Status) -> Style {
    let palette = theme.extended_palette();
    let base = styled(palette.background.base);

    match status {
        Status::Hovered | Status::Pressed | Status::Active => Style {
            background: Some(Background::Color(palette.background.weak.color)),
            ..base
        },
        Status::Disabled => disabled(base),
    }
}

fn styled(pair: palette::Pair) -> Style {
    Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: border::rounded(2),
        ..Style::default()
    }
}

fn disabled(style: Style) -> Style {
    Style {
        background: style
            .background
            .map(|background| background.scale_alpha(0.5)),
        text_color: style.text_color.scale_alpha(0.5),
        ..style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Theme;

    #[test]
    fn test_background_style() {
        let theme = Theme::Dark;
        let style_active = background(&theme, Status::Active);
        let style_hovered = background(&theme, Status::Hovered);
        let style_pressed = background(&theme, Status::Pressed);
        let style_disabled = background(&theme, Status::Disabled);

        assert!(style_active.background.is_some());
        assert!(style_hovered.background.is_some());
        assert!(style_pressed.background.is_some());
        assert!(style_disabled.background.is_some());

        // Hovered/Pressed/Active should have the same background in this implementation
        assert_eq!(style_hovered.background, style_active.background);
        assert_eq!(style_pressed.background, style_active.background);
    }
}
