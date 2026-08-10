use std::error::Error;

use tokio::sync::oneshot;
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

use swawkit_proj::{
    context::EntryContext,
    data_root::DataRootSession,
    host_runtime::{HostRuntimeDocument, HostRuntimeOwner},
    server::{self, ServerEvent},
};

use crate::host_instance::HostInstance;

const STATUS_ID: &str = "tray.status";
const OPEN_ID: &str = "tray.open";
const QUIT_ID: &str = "tray.quit";

#[derive(Debug)]
enum AppEvent {
    Menu(MenuEvent),
    Server(ServerEvent),
}

struct App {
    tray: Option<TrayIcon>,
    status_item: Option<MenuItem>,
    open_item: Option<MenuItem>,
    quit_item: Option<MenuItem>,
    server_url: Option<String>,
    server_error: Option<String>,
    shutdown: Option<oneshot::Sender<()>>,
    browser_opened: bool,
    shutting_down: bool,
    _host_instance: HostInstance,
    host_runtime: HostRuntimeOwner,
}

impl App {
    fn new(
        shutdown: oneshot::Sender<()>,
        host_instance: HostInstance,
        host_runtime: HostRuntimeOwner,
    ) -> Self {
        Self {
            tray: None,
            status_item: None,
            open_item: None,
            quit_item: None,
            server_url: None,
            server_error: None,
            shutdown: Some(shutdown),
            browser_opened: false,
            shutting_down: false,
            _host_instance: host_instance,
            host_runtime,
        }
    }

    fn create_tray(&mut self) -> Result<(), Box<dyn Error>> {
        let (status_text, tooltip) = self.status_text();
        let status = MenuItem::with_id(STATUS_ID, status_text, false, None);
        let open = MenuItem::with_id(
            OPEN_ID,
            "打开控制台",
            self.server_url.is_some() && !self.shutting_down,
            None,
        );
        let quit = MenuItem::with_id(QUIT_ID, "退出", !self.shutting_down, None);
        let menu = Menu::with_items(&[&status, &open, &quit])?;
        let tray = TrayIconBuilder::new()
            .with_tooltip(tooltip)
            .with_icon(solid_icon()?)
            .with_menu(Box::new(menu))
            .build()?;

        self.status_item = Some(status);
        self.open_item = Some(open);
        self.quit_item = Some(quit);
        self.tray = Some(tray);
        Ok(())
    }

    fn status_text(&self) -> (String, String) {
        if self.shutting_down {
            return ("正在停止…".to_owned(), "Swaw Kit — 正在停止".to_owned());
        }
        if self.server_error.is_some() {
            if self.server_url.is_some() {
                return (
                    "在线 — 无法打开浏览器".to_owned(),
                    "Swaw Kit — 无法打开浏览器".to_owned(),
                );
            }
            return (
                "离线 — 服务启动失败".to_owned(),
                "Swaw Kit — 服务启动失败".to_owned(),
            );
        }
        if let Some(url) = &self.server_url {
            return (format!("在线 — {url}"), format!("Swaw Kit — {url}"));
        }
        ("正在启动…".to_owned(), "Swaw Kit — 正在启动".to_owned())
    }

    fn update_tray_status(&self) {
        let (status_text, tooltip) = self.status_text();
        if let Some(status) = &self.status_item {
            status.set_text(status_text);
        }
        if let Some(open) = &self.open_item {
            open.set_enabled(self.server_url.is_some() && !self.shutting_down);
        }
        if let Some(quit) = &self.quit_item {
            quit.set_enabled(!self.shutting_down);
        }
        if let Some(tray) = &self.tray {
            let _ = tray.set_tooltip(Some(tooltip));
        }
    }

    fn server_ready(&mut self, document: HostRuntimeDocument) -> Result<(), String> {
        self.host_runtime
            .publish(&document)
            .map_err(|error| format!("cannot publish the Host runtime endpoint: {error}"))?;
        self.server_url = Some(document.url);
        self.server_error = None;
        if self.shutting_down {
            return Ok(());
        }

        self.update_tray_status();
        if !self.browser_opened {
            self.open_browser()?;
            self.browser_opened = true;
        }
        Ok(())
    }

    fn server_stopped(&mut self, result: Result<(), String>) {
        self.server_url = None;
        self.server_error = result.err();
    }

    fn open_browser(&self) -> Result<(), String> {
        let Some(url) = self.server_url.as_deref() else {
            return Ok(());
        };
        webbrowser::open(url).map_err(|error| format!("cannot open the Web console: {error}"))
    }

    fn request_shutdown(&mut self) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.update_tray_status();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray.is_some() {
            return;
        }
        if let Err(error) = self.create_tray() {
            self.server_error = Some(format!("cannot create the system tray: {error}"));
            self.request_shutdown();
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Menu(event) => match event.id().as_ref() {
                OPEN_ID => {
                    if let Err(error) = self.open_browser() {
                        self.server_error = Some(error);
                        self.update_tray_status();
                    }
                }
                QUIT_ID => {
                    self.request_shutdown();
                }
                _ => {}
            },
            AppEvent::Server(ServerEvent::Ready(document)) => {
                if let Err(error) = self.server_ready(document) {
                    self.server_error = Some(error);
                    self.request_shutdown();
                    event_loop.exit();
                }
            }
            AppEvent::Server(ServerEvent::Stopped(result)) => {
                self.server_stopped(result);
                event_loop.exit();
            }
        }
    }
}

pub fn run(
    context: EntryContext,
    data_root: DataRootSession,
    host_instance: HostInstance,
    host_runtime: HostRuntimeOwner,
) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let menu_proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = menu_proxy.send_event(AppEvent::Menu(event));
    }));

    let (shutdown, shutdown_receiver) = oneshot::channel();
    let server_proxy = event_loop.create_proxy();
    let server_thread = server::spawn(
        context,
        data_root,
        host_runtime.identity(),
        move |event| {
            server_proxy
                .send_event(AppEvent::Server(event))
                .map_err(|_| "application event loop has stopped".to_owned())
        },
        shutdown_receiver,
    )?;
    let mut app = App::new(shutdown, host_instance, host_runtime);

    let event_loop_result = event_loop.run_app(&mut app);
    app.request_shutdown();
    let server_result = server_thread.join();
    event_loop_result?;
    if server_result.is_err() {
        return Err("Axum server thread panicked".into());
    }
    if let Some(error) = app.server_error {
        return Err(format!("Axum server stopped: {error}").into());
    }
    Ok(())
}

fn solid_icon() -> Result<Icon, tray_icon::BadIcon> {
    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 32;
    let mut rgba = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let inset = x < 4 || y < 4 || x >= WIDTH - 4 || y >= HEIGHT - 4;
            let pixel = if inset {
                [18, 52, 86, 255]
            } else {
                [48, 146, 220, 255]
            };
            rgba.extend_from_slice(&pixel);
        }
    }

    Icon::from_rgba(rgba, WIDTH, HEIGHT)
}
