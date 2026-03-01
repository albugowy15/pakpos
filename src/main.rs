use pakpos::app::AppState;

fn main() -> iced::Result {
    iced::application(AppState::default, AppState::update, AppState::view)
        .title(AppState::title)
        .run()
}
