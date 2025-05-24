use std::sync::{Arc, RwLock, RwLockWriteGuard};

use eframe::egui::{self, CornerRadius, Id, RichText, SidePanel};
use egui_dock::{DockArea, DockState, Style};
use futures::{SinkExt, StreamExt};
use lazy_static::lazy_static;

use super::{main::MainWindow, put::PutWindow, fs::FsWindow, stats::StatsWindow, Window, WindowType};


lazy_static! {
    static ref WINDOW_SYSTEM: Arc<RwLock<WindowSystem>> =
        Arc::new(RwLock::new(WindowSystem::default()));
}

pub fn window_system_mut() -> RwLockWriteGuard<'static, WindowSystem> {
    WINDOW_SYSTEM.write().unwrap()
}

lazy_static::lazy_static! {
    static ref NEW_WINDOW_TX: RwLock<Option<futures::channel::mpsc::Sender<WindowType>>>
        = RwLock::new(None);
}

pub fn new_window<T: Into<WindowType> + 'static>(window: T) {
    wasm_bindgen_futures::spawn_local(async move {
        let mut tx_guard = NEW_WINDOW_TX.write().unwrap();
        if let Some(tx) = tx_guard.as_mut() {
            tx.send(window.into()).await.unwrap();
        } else {
            panic!("Game not initialized");
        }
    });
}




struct TabViewer {}

impl egui_dock::TabViewer for TabViewer {
    type Tab = WindowType;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        // Add an icon to the title to indicate the tab type
        let icon = match tab {
            WindowType::Main(_) => "🛸 ",
            WindowType::Put(_) => "📤 ",
            WindowType::Fs(_) => "📁 ",
            WindowType::Stats(_) => "📊 ",
        };

        // Create a compact title that doesn't expand to fill all space
        let title = format!("{}{}", icon, tab.name());
        egui::RichText::new(title).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        tab.draw(ui);
    }

    // Allow all tabs to be closable
    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        true
    }
}

// Static counter to track window positions
lazy_static! {
    static ref WINDOW_COUNTER: RwLock<usize> = RwLock::new(0);
}

pub struct WindowSystem {
    tree: DockState<WindowType>,
    need_focus: Option<(i32, i32)>,
    // chart_window: ChartWindow,
    frame: usize,
}

impl WindowSystem {
    pub fn add_window(&mut self, window: WindowType) {
        // Check if a window with the same name already exists
        if self
            .tree
            .iter_all_tabs()
            .any(|((_, _), w)| w.name() == window.name())
        {
            return;
        }

        let size = Self::default_window_size(&window);

        // Get the current window counter and increment it
        let mut counter = WINDOW_COUNTER.write().unwrap();
        *counter = (*counter + 1) % 10; // Cycle through 10 positions to avoid going off-screen

        // Calculate position with an offset based on the counter
        // Start at [60.0, 20.0] and cascade each window by [20.0, 20.0]
        let position = [60.0 + (*counter as f32 * 20.0), 20.0 + (*counter as f32 * 20.0)];

        // Create a new window
        let surface = self.tree.add_window(vec![window]);

        // Set the window position and size
        self.tree
            .get_window_state_mut(surface)
            .unwrap()
            .set_size(size.into())
            .set_position(position.into());
    }

    pub fn add_main_dock_area(&mut self, window: WindowType) {
        // Add to main surface - this will allow docking
        self.tree.main_surface_mut().push_to_focused_leaf(window);
    }

    fn default_window_size(window_type: &WindowType) -> [f32; 2] {
        match window_type {
            WindowType::Fs(_) => [300.0, 600.0],
            WindowType::Stats(_) => [500.0, 600.0],
            // WindowType::Overview(_) => [300.0, 600.0],
            // WindowType::Ships(_) => [300.0, 600.0],
            // WindowType::Bases(_) => [600.0, 400.0],
            // WindowType::Research(_) => [600.0, 400.0],
            // WindowType::Flights(_) => [600.0, 500.0],
            // WindowType::Orders(_) => [300.0, 400.0],
            // WindowType::Trades(_) => [300.0, 400.0],
            // WindowType::Blueprints(_) => [300.0, 500.0],
            _ => [300.0, 400.0],
        }
    }

    // Get a mutable reference to a specific window type by ID
    pub fn get_window_mut<T: 'static>(&mut self, _window_id: egui::Id) -> Option<&mut T> {
        // For now, we'll just return the first window of the requested type

        // Iterate through all tabs in the tree
        for (_, window_type) in self.tree.iter_all_tabs_mut() {
            // Try to downcast to the requested type based on the window type
            if let WindowType::Fs(fs_window) = window_type {
                // Check if this is the type we're looking for
                if std::any::TypeId::of::<T>() == std::any::TypeId::of::<FsWindow>() {
                    // This is unsafe, but we've verified the type
                    return Some(unsafe { &mut *(fs_window as *mut FsWindow as *mut T) });
                }
            }
            // Add other window types as needed
        }
        None
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        // Change from SidePanel::right to SidePanel::left
        SidePanel::left("left_menu")
            .exact_width(25.0)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);

                    // Helper function to check if window is open
                    let is_window_open = |name: &str| {
                        self.tree
                            .iter_all_tabs()
                            .any(|((_, _), w)| w.name() == name)
                    };

                    // Helper function to create a button with different styling when active
                    let menu_button =
                        |ui: &mut egui::Ui, icon: &str, hover: &str, is_open: bool| {
                            let button = egui::Button::new(if is_open {
                                RichText::new(icon)
                                    .strong()
                                    .background_color(ui.style().visuals.selection.bg_fill)
                            } else {
                                RichText::new(icon)
                            });
                            ui.add(button).on_hover_text(hover)
                        };

                    if menu_button(ui, "🛸", "Main", is_window_open("MutAnt Keys")).clicked() {
                        new_window(MainWindow::new());
                    }

                    // if menu_button(ui, "⚙️", "Tasks", is_window_open("MutAnt Tasks")).clicked() {
                    //     new_window(TasksWindow::new());
                    // }

                    if menu_button(ui, "📤", "Upload", is_window_open("MutAnt Upload")).clicked() {
                        new_window(PutWindow::new());
                    }

                    if menu_button(ui, "📁", "Files", is_window_open("MutAnt Files")).clicked() {
                        new_window(FsWindow::new());
                    }

                    if menu_button(ui, "📊", "Stats", is_window_open("MutAnt Stats")).clicked() {
                        new_window(StatsWindow::new());
                    }

                    // if menu_button(ui, "🚀", "Ships", is_window_open("Ships")).clicked() {
                    //     new_window(ShipsWindow {});
                    // }

                    // if menu_button(ui, "🏠", "Bases", is_window_open("Bases")).clicked() {
                    //     new_window(BasesWindow {});
                    // }

                    // if menu_button(ui, "📊", "Overview", is_window_open("Overview")).clicked() {
                    //     new_window(OverviewWindow::default());
                    // }

                    // if menu_button(ui, "🔬", "Research", is_window_open("Research")).clicked() {
                    //     new_window(ResearchWindow {
                    //         selected_base: None,
                    //     });
                    // }

                    // if menu_button(ui, "📋", "Orders", is_window_open("Orders")).clicked() {
                    //     new_window(OrdersWindow::new())
                    // }

                    // if menu_button(ui, "💱", "Trades", is_window_open("Trades")).clicked() {
                    //     new_window(TradesWindow::new());
                    // }

                    // if menu_button(ui, "📈", "Chart", is_window_open("Chart")).clicked() {
                    //     self.chart_window.id += 1;
                    //     new_window(self.chart_window.clone());
                    // }

                    // if menu_button(ui, "📝", "Blueprint", is_window_open("Blueprint")).clicked() {
                    //     new_window(BlueprintsWindow::new());
                    // }

                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |_ui| {
                        // if ui.button("🚪").on_hover_text("Logout").clicked() {
                        //     // SessionCookie::logout();
                        //     let window = web_sys::window().unwrap();
                        //     window.location().reload().unwrap();
                        // }
                    });
                });
            });

        // Create a custom style for the dock area
        let mut style = Style::from_egui(ui.ctx().style().as_ref());

        // Configure the style to make docking more visible and user-friendly
        style.tab_bar.fill_tab_bar = false; // Don't fill the entire tab bar
        // Keep the original background color for the tab bar
        style.tab_bar.bg_fill = egui::Color32::from_rgb(40, 40, 40);
        style.tab.tab_body.bg_fill = egui::Color32::from_rgb(40, 40, 40);

        style.tab.active.bg_fill = egui::Color32::from_rgb(50, 50, 50);
        style.tab.focused.bg_fill = egui::Color32::from_rgb(50, 50, 50);


        // Use darker colors for the horizontal line to increase contrast
        style.tab_bar.hline_color = egui::Color32::from_rgb(70, 70, 70); // Darker line color
        style.tab_bar.corner_radius = CornerRadius::default();

        // Create the dock area with docking enabled
        DockArea::new(&mut self.tree)
            .style(style)
            .id(Id::new("egui_dock::DockArea"))
            .show_inside(ui, &mut TabViewer {});

        self.frame += 1;

        if self.frame >= 10 {
            let window = web_sys::window().unwrap();

            // Try to serialize the dock state
            match serde_json::to_string(&self.tree) {
                Ok(serie) => {
                    // Try to save to localStorage, but handle quota exceeded errors gracefully
                    if let Some(storage) = window.local_storage().ok().flatten() {
                        // Attempt to set the value, but don't unwrap to avoid panicking
                        if let Err(e) = storage.set("egui_dock::DockArea", &serie) {
                            log::warn!("Failed to save window state to localStorage: {:?}", e);
                            // Continue without crashing - the user will just lose window state
                        }
                    }
                },
                Err(e) => {
                    log::warn!("Failed to serialize window state: {:?}", e);
                    // Continue without crashing
                }
            }

            self.frame = 0;
        }
    }


    pub fn from_memory(user_id: String) -> Self {
        // Reset the window counter when loading from memory
        *WINDOW_COUNTER.write().unwrap() = 0;

        let window = match web_sys::window() {
            Some(w) => w,
            None => return Self::default_with_main_window(),
        };

        let storage = match window.local_storage().ok().flatten() {
            Some(s) => s,
            None => return Self::default_with_main_window(),
        };

        let key = format!("egui_dock::DockArea{}", user_id);
        let data = match storage.get(key.as_str()).ok().flatten() {
            Some(d) => d,
            None => return Self::default_with_main_window(),
        };

        match serde_json::from_str::<DockState<WindowType>>(&data) {
            Ok(tree) => Self {
                tree,
                ..Default::default()
            },
            Err(e) => {
                log::warn!("Failed to deserialize window state: {:?}", e);
                Self::default_with_main_window()
            }
        }
    }

    fn default_with_main_window() -> Self {
        // Reset the window counter when initializing the window system
        *WINDOW_COUNTER.write().unwrap() = 0;

        // Create a new DockState with the main window
        // This will create a main surface with a single tab that can be used for docking
        let tree = DockState::new(vec![MainWindow::default().into()]);

        log::info!("Initialized main window for docking");

        Self {
            tree,
            ..Default::default()
        }
    }
}

impl Default for WindowSystem {
    fn default() -> Self {
        // Reset the window counter when creating a new window system
        *WINDOW_COUNTER.write().unwrap() = 0;

        let tree = DockState::new(vec![]);

        Self {
            frame: 0,
            tree,
            need_focus: None,
        }
    }
}

pub async fn init_window_system() {
    let (new_window_tx, mut new_window_rx) = futures::channel::mpsc::channel(1);
    // let (request_focus_tx, mut request_focus_rx) = futures::channel::mpsc::channel(1);

    *NEW_WINDOW_TX.write().unwrap() = Some(new_window_tx);
    // *REQUEST_FOCUS_TX.write().unwrap() = Some(request_focus_tx);

    wasm_bindgen_futures::spawn_local(async move {
        while let Some(window) = new_window_rx.next().await {
            window_system_mut().add_window(window);
        }
    });

    // wasm_bindgen_futures::spawn_local(async move {
    //     while let Some(pos) = request_focus_rx.next().await {
    //         window_system_mut().request_focus(pos);
    //     }
    // });
}

// pub fn focus_first_base() {
//     let game = game();

//     if let Some(base) = game.bases.iter().next() {
//         request_focus(base.1);

//         return;
//     } else if let Some(ship) = game.ships.iter().next() {
//         let Some(entity_id) = &ship.1.position_id else {
//             return;
//         };

//         let entity = game.get_entity(&entity_id).unwrap();

//         request_focus(&entity);

//         return;
//     }

//     GAME_DATA.read().planets.iter().next().map(|(_, planet)| {
//         request_focus(planet);
//     });
// }
