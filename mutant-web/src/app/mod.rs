// use bevy_egui::egui::{self};
use eframe::egui;
use serde::{Deserialize, Serialize};

// mod base;
// mod bases;
// mod blueprint;
// mod blueprints;
// mod buildings;
// mod chart;
mod components;
pub mod fs_window; // Renamed from fs.rs
pub mod fs; // New directory module
// mod flights;
// mod infobar;
// mod inventory;
// pub mod loading;
// pub mod login;
pub mod notifications;
// mod orders;
// mod overview;
// pub mod planet;
// mod planet_list;
// mod research;
// mod ship;
// mod ships;
pub mod stats;
// mod trades;
pub mod theme;

mod client_manager;
pub mod context;

pub mod put;
// mod tasks;
pub mod window_system;
pub mod colony_window;
pub mod download_window;
pub mod onboarding;

// Re-export the dynamic URL function
pub use client_manager::default_ws_url as DEFAULT_WS_URL;


pub use context::context;

use window_system::init_window_system;
// pub use client_manager::DEFAULT_WS_URL;

// pub use infobar::InfobarWindow;
// pub use research::ResearchWindow;

pub trait Window: Send + Sync {
    fn name(&self) -> String;
    fn draw(&mut self, ui: &mut egui::Ui);
}

#[derive(Clone, Serialize, Deserialize)]
pub enum WindowType {
    // Tasks(tasks::TasksWindow),
    Put(put::PutWindow),
    Fs(fs_window::FsWindow), // Updated path
    Stats(stats::StatsWindow),
    Colony(colony_window::ColonyWindow),
    // Bases(bases::BasesWindow),
    // Base(base::BaseWindow),
    // Buildings(buildings::BuildingsWindow),
    // Chart(chart::ChartWindow),
    // Flights(flights::FlightsWindow),
    // Orders(orders::OrdersWindow),
    // Overview(overview::OverviewWindow),
    // Research(research::ResearchWindow),
    // Ships(ships::ShipsWindow),
    // Ship(ship::ShipWindow),
    // Trades(trades::TradesWindow),
    // Planet(planet::PlanetWindow),
    // PlanetList(planet_list::PlanetListWindow),
    // Blueprints(blueprints::BlueprintsWindow),
    // Blueprint(blueprint::BlueprintWindow),
}

impl Window for WindowType {
    fn name(&self) -> String {
        match self {
            // Self::Tasks(window) => window.name(),
            Self::Put(window) => window.name(),
            Self::Fs(window) => window.name(),
            Self::Stats(window) => window.name(),
            Self::Colony(window) => window.name(),
            // Self::Bases(window) => window.name(),
            // Self::Base(window) => window.name(),
            // Self::Buildings(window) => window.name(),
            // Self::Chart(window) => window.name(),
            // Self::Flights(window) => window.name(),
            // Self::Orders(window) => window.name(),
            // Self::Overview(window) => window.name(),
            // Self::Research(window) => window.name(),
            // Self::Ships(window) => window.name(),
            // Self::Ship(window) => window.name(),
            // Self::Trades(window) => window.name(),
            // Self::Planet(window) => window.name(),
            // Self::PlanetList(window) => window.name(),
            // Self::Blueprints(window) => window.name(),
            // Self::Blueprint(window) => window.name(),
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        match self {
            // Self::Tasks(window) => window.draw(ui),
            Self::Put(window) => window.draw(ui),
            Self::Fs(window) => window.draw(ui),
            Self::Stats(window) => window.draw(ui),
            Self::Colony(window) => window.draw(ui),
            // Self::Bases(window) => window.draw(ui),
            // Self::Base(window) => window.draw(ui),
            // Self::Buildings(window) => window.draw(ui),
            // Self::Chart(window) => window.draw(ui),
            // Self::Flights(window) => window.draw(ui),
            // Self::Orders(window) => window.draw(ui),
            // Self::Overview(window) => window.draw(ui),
            // Self::Research(window) => window.draw(ui),
            // Self::Ships(window) => window.draw(ui),
            // Self::Ship(window) => window.draw(ui),
            // Self::Trades(window) => window.draw(ui),
            // Self::Planet(window) => window.draw(ui),
            // Self::PlanetList(window) => window.draw(ui),
            // Self::Blueprints(window) => window.draw(ui),
            // Self::Blueprint(window) => window.draw(ui),
        }
    }
}



// impl From<tasks::TasksWindow> for WindowType {
//     fn from(window: tasks::TasksWindow) -> Self {
//         Self::Tasks(window)
//     }
// }

impl From<put::PutWindow> for WindowType {
    fn from(window: put::PutWindow) -> Self {
        Self::Put(window)
    }
}

impl From<fs_window::FsWindow> for WindowType {
    fn from(window: fs_window::FsWindow) -> Self {
        Self::Fs(window)
    }
}

impl From<stats::StatsWindow> for WindowType {
    fn from(window: stats::StatsWindow) -> Self {
        Self::Stats(window)
    }
}

impl From<colony_window::ColonyWindow> for WindowType {
    fn from(window: colony_window::ColonyWindow) -> Self {
        Self::Colony(window)
    }
}

// impl From<bases::BasesWindow> for WindowType {
//     fn from(window: bases::BasesWindow) -> Self {
//         Self::Bases(window)
//     }
// }

// impl From<base::BaseWindow> for WindowType {
//     fn from(window: base::BaseWindow) -> Self {
//         Self::Base(window)
//     }
// }

// impl From<buildings::BuildingsWindow> for WindowType {
//     fn from(window: buildings::BuildingsWindow) -> Self {
//         Self::Buildings(window)
//     }
// }

// impl From<chart::ChartWindow> for WindowType {
//     fn from(window: chart::ChartWindow) -> Self {
//         Self::Chart(window)
//     }
// }

// impl From<flights::FlightsWindow> for WindowType {
//     fn from(window: flights::FlightsWindow) -> Self {
//         Self::Flights(window)
//     }
// }

// impl From<orders::OrdersWindow> for WindowType {
//     fn from(window: orders::OrdersWindow) -> Self {
//         Self::Orders(window)
//     }
// }

// impl From<overview::OverviewWindow> for WindowType {
//     fn from(window: overview::OverviewWindow) -> Self {
//         Self::Overview(window)
//     }
// }

// impl From<research::ResearchWindow> for WindowType {
//     fn from(window: research::ResearchWindow) -> Self {
//         Self::Research(window)
//     }
// }

// impl From<ships::ShipsWindow> for WindowType {
//     fn from(window: ships::ShipsWindow) -> Self {
//         Self::Ships(window)
//     }
// }

// impl From<ship::ShipWindow> for WindowType {
//     fn from(window: ship::ShipWindow) -> Self {
//         Self::Ship(window)
//     }
// }

// impl From<trades::TradesWindow> for WindowType {
//     fn from(window: trades::TradesWindow) -> Self {
//         Self::Trades(window)
//     }
// }

// impl From<planet::PlanetWindow> for WindowType {
//     fn from(window: planet::PlanetWindow) -> Self {
//         Self::Planet(window)
//     }
// }

// impl From<planet_list::PlanetListWindow> for WindowType {
//     fn from(window: planet_list::PlanetListWindow) -> Self {
//         Self::PlanetList(window)
//     }
// }

// impl From<blueprints::BlueprintsWindow> for WindowType {
//     fn from(window: blueprints::BlueprintsWindow) -> Self {
//         Self::Blueprints(window)
//     }
// }

// impl From<blueprint::BlueprintWindow> for WindowType {
//     fn from(window: blueprint::BlueprintWindow) -> Self {
//         Self::Blueprint(window)
//     }
// }

pub async fn init() {
    // Get the shared context
    let ctx = context::context();

    // Fetch keys using the context
    let _ = ctx.list_keys().await;

    // Initialize the window system (for secondary windows only)
    init_window_system().await;

    // Note: FsWindow is now rendered directly in MyApp, not through the window system
    // The window system is now only used for secondary windows that spawn from the FS window

    // Log that the initialization is complete
    log::info!("App initialized - FS window will be rendered directly, window system ready for secondary windows");
}
