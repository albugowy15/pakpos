use pakpos::app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .resizable(true)
        .decorations(true)
        .run()
}
