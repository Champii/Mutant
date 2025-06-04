use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use eframe::egui::{self, Id, RichText, SidePanel};
use egui_dock::{DockArea, DockState};
use futures::{SinkExt, StreamExt};
use lazy_static::lazy_static;

use super::{put::PutWindow, fs_window::FsWindow, fs::viewer_tab::FileViewerTab, stats::StatsWindow, colony_window::ColonyWindow, Window, WindowType, theme::MutantColors};

/// Unified tab type that can hold any kind of tab in the dock system
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum UnifiedTab {
    Put(PutWindow),
    Fs(FsWindow),
    Stats(StatsWindow),
    Colony(ColonyWindow),
    FileViewer(FileViewerTab),
}

impl UnifiedTab {
    pub fn name(&self) -> String {
        match self {
            UnifiedTab::Put(w) => w.name(),
            UnifiedTab::Fs(w) => w.name(),
            UnifiedTab::Stats(w) => w.name(),
            UnifiedTab::Colony(w) => w.name(),
            UnifiedTab::FileViewer(tab) => tab.file.key.clone(),
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        match self {
            UnifiedTab::Put(w) => w.draw(ui),
            UnifiedTab::Fs(w) => w.draw(ui), // Still use regular draw for now
            UnifiedTab::Stats(w) => w.draw(ui),
            UnifiedTab::Colony(w) => w.draw(ui),
            UnifiedTab::FileViewer(tab) => tab.draw(ui),
        }
    }

// draw_with_dock_state method removed - using simple dual dock approach
}

// Conversion from WindowType to UnifiedTab
impl From<WindowType> for UnifiedTab {
    fn from(window_type: WindowType) -> Self {
        match window_type {
            WindowType::Put(w) => UnifiedTab::Put(w),
            WindowType::Fs(w) => UnifiedTab::Fs(w),
            WindowType::Stats(w) => UnifiedTab::Stats(w),
            WindowType::Colony(w) => UnifiedTab::Colony(w),
        }
    }
}

// Conversion from FileViewerTab to UnifiedTab
impl From<FileViewerTab> for UnifiedTab {
    fn from(tab: FileViewerTab) -> Self {
        UnifiedTab::FileViewer(tab)
    }
}

lazy_static! {
    static ref WINDOW_SYSTEM: Arc<RwLock<WindowSystem>> =
        Arc::new(RwLock::new(WindowSystem::default()));
}

pub fn window_system_mut() -> RwLockWriteGuard<'static, WindowSystem> {
    WINDOW_SYSTEM.write().unwrap()
}

pub fn window_system() -> RwLockReadGuard<'static, WindowSystem> {
    WINDOW_SYSTEM.read().unwrap()
}

lazy_static::lazy_static! {
    static ref NEW_WINDOW_TX: RwLock<Option<futures::channel::mpsc::Sender<WindowType>>>
        = RwLock::new(None);
}

fn new_window_tx() -> futures::channel::mpsc::Sender<WindowType> {
    let guard = NEW_WINDOW_TX.read().unwrap();
    match guard.as_ref() {
        Some(x) => x.clone(),
        None => panic!("Game not initialized"),
    }
}

pub fn new_window<T: Into<WindowType> + 'static>(window: T) {
    wasm_bindgen_futures::spawn_local(async move {
        let mut tx = new_window_tx();
        tx.send(window.into()).await.unwrap();
    });
}

/// Add a file viewer tab to the unified dock system
pub fn new_file_viewer_tab(tab: FileViewerTab) {
    log::info!("Adding new file viewer tab for: {}", tab.file.key);

    let mut window_system = window_system_mut();

    // Check if a tab for this file already exists
    let tab_exists = window_system.tree.iter_all_tabs().any(|(_, unified_tab)| {
        if let UnifiedTab::FileViewer(existing_tab) = unified_tab {
            existing_tab.file.key == tab.file.key
        } else {
            false
        }
    });

    if !tab_exists {
        // Ensure we have a valid dock tree
        let has_tabs = window_system.tree.iter_all_tabs().next().is_some();

        if !has_tabs {
            // If the tree has no tabs, create an empty main surface
            window_system.tree = DockState::new(vec![]);
        }

        // Add the file viewer tab to the main surface
        window_system.tree.main_surface_mut().push_to_focused_leaf(UnifiedTab::FileViewer(tab));
        log::info!("Successfully added file viewer tab to dock system");
    } else {
        log::info!("Tab for file {} already exists", tab.file.key);
        // TODO: Focus the existing tab
    }
}




pub struct UnifiedTabViewer {}

impl egui_dock::TabViewer for UnifiedTabViewer {
    type Tab = UnifiedTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        // Add an icon to the title to indicate the tab type
        let (icon, title_text) = match tab {
            UnifiedTab::Put(_) => ("📤 ", tab.name()),
            UnifiedTab::Fs(_) => ("📁 ", tab.name()),
            UnifiedTab::Stats(_) => ("📊 ", tab.name()),
            UnifiedTab::Colony(_) => ("🌐 ", tab.name()),
            UnifiedTab::FileViewer(file_tab) => {
                // Get the file name from the path
                let file_name = std::path::Path::new(&file_tab.file.key)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| file_tab.file.key.clone());

                // Add an icon based on the file type
                let file_icon = match &file_tab.file_type {
                    Some(super::components::multimedia::FileType::Text) => "📄",
                    Some(super::components::multimedia::FileType::Code(_)) => "📝",
                    Some(super::components::multimedia::FileType::Image) => "🖼️",
                    Some(super::components::multimedia::FileType::Video) => "🎬",
                    Some(super::components::multimedia::FileType::Other) | None => "📄",
                };

                // Add a modified indicator if the file has been modified
                let modified_indicator = if file_tab.file_modified { "* " } else { "" };

                (file_icon, format!("{}{}", modified_indicator, file_name))
            }
        };

        // Create a compact title with professional styling
        let title = format!("{}{}", icon, title_text);
        egui::RichText::new(title)
            .size(12.0) // Consistent with filesystem tree
            .color(super::theme::MutantColors::TEXT_PRIMARY)
            .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        // Set proper background for the content area
        let frame = egui::Frame::new()
            .fill(super::theme::MutantColors::BACKGROUND_DARK)
            .inner_margin(egui::Margin::same(8));

        frame.show(ui, |ui| {
            // For FsWindow tabs, we need to use the special draw method that avoids deadlock
            // For other tabs, use the regular draw method
            match tab {
                UnifiedTab::Fs(_) => {
                    // Use regular draw for now - the shared dock functionality will be handled differently
                    tab.draw(ui);
                }
                _ => {
                    tab.draw(ui);
                }
            }
        });
    }

    // Allow all tabs to be closable
    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        true
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        match tab {
            UnifiedTab::FileViewer(file_tab) => {
                // Cleanup video player if it exists
                file_tab.cleanup_video_player();
                true
            }
            _ => true,
        }
    }
}

// Static counter to track window positions
lazy_static! {
    static ref WINDOW_COUNTER: RwLock<usize> = RwLock::new(0);
}

// Global dock area ID counter to ensure unique IDs across all dock areas
lazy_static! {
    static ref DOCK_AREA_ID_COUNTER: RwLock<usize> = RwLock::new(0);
}

/// Generate a unique dock area ID
pub fn generate_unique_dock_area_id() -> String {
    let mut counter = DOCK_AREA_ID_COUNTER.write().unwrap();
    *counter += 1;
    format!("mutant_dock_area_{}", *counter)
}

pub struct WindowSystem {
    tree: DockState<UnifiedTab>,
    need_focus: Option<(i32, i32)>,
    // chart_window: ChartWindow,
    frame: usize,
    /// Unique dock area ID for this window system
    dock_area_id: String,
}

impl WindowSystem {
    /// Get a mutable reference to the dock tree
    pub fn tree_mut(&mut self) -> &mut DockState<UnifiedTab> {
        &mut self.tree
    }

    /// Get a reference to the dock tree
    pub fn tree(&self) -> &DockState<UnifiedTab> {
        &self.tree
    }

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
        let surface = self.tree.add_window(vec![window.into()]);

        // Set the window position and size
        self.tree
            .get_window_state_mut(surface)
            .unwrap()
            .set_size(size.into())
            .set_position(position.into());
    }

    pub fn add_main_dock_area(&mut self, window: WindowType) {
        // Add to main surface - this will allow docking
        self.tree.main_surface_mut().push_to_focused_leaf(window.into());
    }

    fn default_window_size(window_type: &WindowType) -> [f32; 2] {
        match window_type {
            WindowType::Fs(_) => [300.0, 600.0],
            WindowType::Stats(_) => [500.0, 600.0],
            WindowType::Colony(_) => [800.0, 600.0], // Wider for two-column layout
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
        for (_, unified_tab) in self.tree.iter_all_tabs_mut() {
            // Try to downcast to the requested type based on the unified tab type
            if let UnifiedTab::Fs(fs_window) = unified_tab {
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

    // Get a read-only reference to a specific window type by ID
    pub fn get_window<T: 'static>(&self, _window_id: egui::Id) -> Option<&T> {
        // For now, we'll just return the first window of the requested type

        // Iterate through all tabs in the tree
        for (_, unified_tab) in self.tree.iter_all_tabs() {
            // Try to downcast to the requested type based on the unified tab type
            if let UnifiedTab::Fs(fs_window) = unified_tab {
                // Check if this is the type we're looking for
                if std::any::TypeId::of::<T>() == std::any::TypeId::of::<FsWindow>() {
                    // This is unsafe, but we've verified the type
                    return Some(unsafe { &*(fs_window as *const FsWindow as *const T) });
                }
            }
            // Add other window types as needed
        }
        None
    }

    /// Draw only the dock area without the left menu (for use with external menu)
    pub fn draw_dock_only(&mut self, ui: &mut egui::Ui) {
        // Don't override the style - let it inherit from the root MutAnt theme
        // Create the dock area with docking enabled
        DockArea::new(&mut self.tree)
            .id(Id::new(&self.dock_area_id))
            .show_inside(ui, &mut UnifiedTabViewer {});

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

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        // Change from SidePanel::right to SidePanel::left
        SidePanel::left("left_menu")
            .exact_width(60.0)
            .frame(egui::Frame::new()
                .fill(MutantColors::BACKGROUND_MEDIUM)
                .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_DARK)))
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(15.0);

                    // Helper function to check if window is open
                    let is_window_open = |name: &str| {
                        self.tree
                            .iter_all_tabs()
                            .any(|((_, _), w)| w.name() == name)
                    };

                    // Helper function to create a button with different styling when active
                    let menu_button =
                        |ui: &mut egui::Ui, icon: &str, hover: &str, is_open: bool| {
                            let button = if is_open {
                                egui::Button::new(
                                    RichText::new(icon)
                                        .size(18.0)
                                        .strong()
                                        .color(MutantColors::BACKGROUND_DARK)
                                )
                                .fill(MutantColors::ACCENT_ORANGE)
                                .stroke(egui::Stroke::new(2.0, MutantColors::ACCENT_ORANGE))
                                .min_size([40.0, 40.0].into())
                            } else {
                                egui::Button::new(
                                    RichText::new(icon)
                                        .size(16.0)
                                        .color(MutantColors::TEXT_SECONDARY)
                                )
                                .fill(MutantColors::SURFACE)
                                .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
                                .min_size([40.0, 40.0].into())
                            };
                            ui.add(button).on_hover_text(hover)
                        };



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

                    if menu_button(ui, "🌐", "Colony", is_window_open("Colony")).clicked() {
                        new_window(ColonyWindow::default());
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

        // Don't override the style - let it inherit from the root MutAnt theme
        // Create the dock area with docking enabled
        DockArea::new(&mut self.tree)
            .id(Id::new(&self.dock_area_id))
            .show_inside(ui, &mut UnifiedTabViewer {});

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

        match serde_json::from_str::<DockState<UnifiedTab>>(&data) {
            Ok(tree) => Self {
                tree,
                frame: 0,
                need_focus: None,
                dock_area_id: generate_unique_dock_area_id(),
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

        // Create an empty DockState
        let tree = DockState::new(vec![]);

        log::info!("Initialized empty dock system");

        Self {
            tree,
            frame: 0,
            need_focus: None,
            dock_area_id: generate_unique_dock_area_id(),
        }
    }
}

impl Default for WindowSystem {
    fn default() -> Self {
        // Reset the window counter when creating a new window system
        *WINDOW_COUNTER.write().unwrap() = 0;

        // Create an empty DockState
        let tree = DockState::new(vec![]);

        log::info!("Initialized default WindowSystem");

        Self {
            frame: 0,
            tree,
            need_focus: None,
            dock_area_id: generate_unique_dock_area_id(),
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

impl WindowSystem {
    /// Find a FileViewerTab by key (read-only)
    pub fn find_file_viewer_tab(&self, key: &str) -> Option<&FileViewerTab> {
        for (_, unified_tab) in self.tree.iter_all_tabs() {
            if let UnifiedTab::FileViewer(tab) = unified_tab {
                if tab.file.key == key {
                    return Some(tab);
                }
            }
        }
        None
    }

    /// Find a FileViewerTab by key (mutable)
    pub fn find_file_viewer_tab_mut(&mut self, key: &str) -> Option<&mut FileViewerTab> {
        for (_, unified_tab) in self.tree.iter_all_tabs_mut() {
            if let UnifiedTab::FileViewer(tab) = unified_tab {
                if tab.file.key == key {
                    return Some(tab);
                }
            }
        }
        None
    }
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
