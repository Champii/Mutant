use crate::app::{Window, context::context};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// Global state for storing user contact info responses
lazy_static::lazy_static! {
    static ref USER_CONTACT_RESPONSES: Arc<Mutex<HashMap<String, UserContactInfo>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref CONTENT_LIST_RESPONSES: Arc<Mutex<HashMap<String, Vec<ContentItem>>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref CONTACT_LIST_RESPONSES: Arc<Mutex<HashMap<String, Vec<String>>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref COLONY_PROGRESS_EVENTS: Arc<Mutex<Vec<ColonyProgressEvent>>> = Arc::new(Mutex::new(Vec::new()));
}

#[derive(Clone, Debug)]
pub struct ColonyProgressEvent {
    pub event: mutant_protocol::ColonyEvent,
    pub operation_id: Option<String>,
}

/// The Colony window for managing contacts and discovering content
#[derive(Clone, Serialize, Deserialize)]
pub struct ColonyWindow {
    /// List of contacts (pod addresses)
    contacts: Vec<Contact>,
    /// Available content from all contacts
    content_list: Vec<ContentItem>,
    /// Content organized by pods
    pod_content: Vec<PodContent>,
    /// Search query for filtering content
    search_query: String,
    /// New contact input fields
    new_contact_address: String,
    new_contact_name: String,
    /// UI state
    #[serde(skip)]
    is_syncing: bool,
    /// User's own contact information
    #[serde(skip)]
    user_contact_info: Option<UserContactInfo>,
    #[serde(skip)]
    is_loading_user_contact: bool,
    /// Flag to trigger contact info reload
    #[serde(skip)]
    should_load_contact_info: bool,
    /// Content list loading state
    #[serde(skip)]
    is_loading_content: bool,
    /// Flag to trigger content list reload
    #[serde(skip)]
    should_load_content: bool,
    /// Contact list loading state
    #[serde(skip)]
    is_loading_contacts: bool,
    /// Flag to trigger contact list reload
    #[serde(skip)]
    should_load_contacts: bool,
    /// Colony operation progress events
    #[serde(skip)]
    progress_events: Vec<ColonyProgressEvent>,
    /// Current operation status message
    #[serde(skip)]
    current_operation_status: Option<String>,
    /// Current operation progress (0.0 to 1.0)
    #[serde(skip)]
    current_progress: f32,
    /// Whether an operation is currently in progress
    #[serde(skip)]
    operation_in_progress: bool,
    /// Whether the contacts section is expanded (visible)
    #[serde(skip)]
    contacts_section_expanded: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Contact {
    pub pod_address: String,
    pub name: Option<String>,
    pub last_synced: Option<String>,
}

#[derive(Clone)]
pub struct UserContactInfo {
    pub contact_address: String,
    pub contact_type: String,
    pub display_name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ContentItem {
    pub title: String,
    pub description: Option<String>,
    pub content_type: String,
    pub address: String,
    pub source_contact: String,
    pub size: Option<u64>,
    pub date_created: Option<String>,
}

/// Structure to organize content by pods
#[derive(Clone, Serialize, Deserialize)]
pub struct PodContent {
    pub pod_address: String,
    pub pod_name: Option<String>,
    pub content_items: Vec<ContentItem>,
    pub is_expanded: bool,
}

impl Default for ColonyWindow {
    fn default() -> Self {
        Self {
            contacts: Vec::new(),
            content_list: Vec::new(),
            pod_content: Vec::new(),
            search_query: String::new(),
            new_contact_address: String::new(),
            new_contact_name: String::new(),
            is_syncing: false,
            user_contact_info: None,
            is_loading_user_contact: false,
            should_load_contact_info: true,
            is_loading_content: false,
            should_load_content: true,
            is_loading_contacts: false,
            should_load_contacts: true,
            progress_events: Vec::new(),
            current_operation_status: None,
            current_progress: 0.0,
            operation_in_progress: false,
            contacts_section_expanded: true, // Show contacts by default
        }
    }
}

impl Window for ColonyWindow {
    fn name(&self) -> String {
        "Colony".to_string()
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        // Check for colony progress events
        self.check_progress_events();

        // Check for colony progress events first
        self.check_progress_events();

        // Auto-load user contact info on first draw if not already loaded/loading
        if self.should_load_contact_info && !self.is_loading_user_contact {
            self.load_user_contact_info();
            self.should_load_contact_info = false;
        }

        // Auto-load content list on first draw if not already loaded/loading
        if self.should_load_content && !self.is_loading_content {
            self.load_content_list();
            self.should_load_content = false;
        }

        // Auto-load contacts on first draw if not already loaded/loading
        if self.should_load_contacts && !self.is_loading_contacts {
            self.load_contacts();
            self.should_load_contacts = false;
        }

        // Check for completed user contact info response
        if self.is_loading_user_contact {
            if let Ok(responses) = USER_CONTACT_RESPONSES.lock() {
                if let Some(contact_info) = responses.get("user_contact") {
                    self.user_contact_info = Some(contact_info.clone());
                    self.is_loading_user_contact = false;
                    // Remove the response from the global state
                    drop(responses);
                    if let Ok(mut responses) = USER_CONTACT_RESPONSES.lock() {
                        responses.remove("user_contact");
                    }
                } else if responses.contains_key("user_contact_error") {
                    // Handle error case
                    self.is_loading_user_contact = false;
                    drop(responses);
                    if let Ok(mut responses) = USER_CONTACT_RESPONSES.lock() {
                        responses.remove("user_contact_error");
                    }
                }
            }
        }

        // Check for completed content list response
        if self.is_loading_content {
            if let Ok(responses) = CONTENT_LIST_RESPONSES.lock() {
                if let Some(content_list) = responses.get("content_list") {
                    self.content_list = content_list.clone();
                    self.organize_content_by_pods();
                    self.is_loading_content = false;
                    // Remove the response from the global state
                    drop(responses);
                    if let Ok(mut responses) = CONTENT_LIST_RESPONSES.lock() {
                        responses.remove("content_list");
                    }
                } else if responses.contains_key("content_list_error") {
                    // Handle error case
                    self.is_loading_content = false;
                    drop(responses);
                    if let Ok(mut responses) = CONTENT_LIST_RESPONSES.lock() {
                        responses.remove("content_list_error");
                    }
                }
            }
        }

        // Check for completed contact list response
        if self.is_loading_contacts {
            if let Ok(responses) = CONTACT_LIST_RESPONSES.lock() {
                if let Some(contact_addresses) = responses.get("contact_list") {
                    log::debug!(
                        "Processing contact list response with {} addresses",
                        contact_addresses.len()
                    );
                    log::debug!("Contact addresses from daemon: {:?}", contact_addresses);

                    // Convert contact addresses to Contact structs
                    self.contacts = contact_addresses
                        .iter()
                        .map(|address| Contact {
                            pod_address: address.clone(),
                            name: None,        // We don't have names from the daemon yet
                            last_synced: None, // No sync info from daemon yet
                        })
                        .collect();
                    self.is_loading_contacts = false;
                    log::info!("UI: Loaded {} contacts from daemon", self.contacts.len());

                    // Remove the response from the global state
                    drop(responses);
                    if let Ok(mut responses) = CONTACT_LIST_RESPONSES.lock() {
                        responses.remove("contact_list");
                        log::debug!("Cleaned up contact_list from global state");
                    }
                } else if responses.contains_key("contact_list_error") {
                    // Handle error case
                    log::warn!("Contact list loading failed, resetting loading state");
                    self.is_loading_contacts = false;
                    drop(responses);
                    if let Ok(mut responses) = CONTACT_LIST_RESPONSES.lock() {
                        responses.remove("contact_list_error");
                    }
                }
            } else {
                log::error!("Failed to acquire lock on CONTACT_LIST_RESPONSES during UI update");
            }
        }
        // Reserve space for the footer (30px) and use the remaining space for content
        let footer_height = 18.0;
        let available_height = ui.available_height();
        let content_height = available_height - footer_height - 40.0; // 10px for separator

        let available_width = ui.available_width();
        let content_width = available_width - 00.0; // Reduced padding to prevent overflow

        // Use a vertical layout for the main content with reserved space
        ui.allocate_ui_with_layout(
            egui::Vec2::new(content_width, content_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
            ui.set_max_height(content_height);
            ui.set_max_width(content_width);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
            // Improved header section with better organization
            ui.vertical(|ui| {
                // Main title and controls row
                ui.horizontal(|ui| {
                    // Title with icon
                    ui.label(
                        egui::RichText::new("🌐 Colony")
                            .heading()
                            .strong()
                            .color(super::theme::MutantColors::ACCENT_ORANGE)
                    );

                    ui.label(
                        egui::RichText::new("Content Discovery Network")
                            .size(14.0)
                            .color(super::theme::MutantColors::TEXT_SECONDARY)
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Sync button with better styling
                        let sync_button = ui.add_enabled(
                            !self.is_syncing,
                            egui::Button::new(
                                egui::RichText::new(if self.is_syncing { "⏳ Syncing..." } else { "🔄 Sync All" })
                                    .strong()
                            )
                            .fill(super::theme::MutantColors::ACCENT_ORANGE)
                            .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::ACCENT_ORANGE))
                        );

                        if sync_button.clicked() {
                            self.sync_all_contacts();
                        }

                        // Toggle button for contacts section with better styling
                        let contacts_icon = if self.contacts_section_expanded { "👥" } else { "👤" };
                        let contacts_text = if self.contacts_section_expanded { "Hide Contacts" } else { "Show Contacts" };
                        let contacts_button = ui.button(
                            egui::RichText::new(format!("{} {}", contacts_icon, contacts_text))
                                .color(super::theme::MutantColors::TEXT_PRIMARY)
                        );
                        if contacts_button.clicked() {
                            self.contacts_section_expanded = !self.contacts_section_expanded;
                        }
                    });
                });

                ui.add_space(8.0);

                // User contact info card (more prominent)
                if let Some(user_info) = &self.user_contact_info {
                    egui::Frame::new()
                        .fill(super::theme::MutantColors::SURFACE)
                        .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_MEDIUM))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .corner_radius(6.0)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                // Header row with label and copy button
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("📡 Your Colony Address:")
                                            .size(12.0)
                                            .color(super::theme::MutantColors::TEXT_SECONDARY)
                                    );

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button(
                                            egui::RichText::new("📋 Copy")
                                                .color(super::theme::MutantColors::TEXT_PRIMARY)
                                        ).clicked() {
                                            ui.ctx().copy_text(user_info.contact_address.clone());
                                            log::info!("Attempted to copy user contact address to clipboard: {}", user_info.contact_address);
                                        }
                                    });
                                });

                                // Address on separate line to prevent overflow
                                ui.add_space(1.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        egui::RichText::new(&user_info.contact_address)
                                            .strong()
                                            .family(egui::FontFamily::Monospace)
                                            .size(10.0)
                                            .color(super::theme::MutantColors::ACCENT_BLUE)
                                    );
                                });
                            });
                        });
                }
            });

            ui.separator();

            // Conditionally show contacts panel based on toggle state
            if self.contacts_section_expanded {
                // Two-column layout using SidePanel for better space management
                egui::SidePanel::left("colony_contacts_panel")
                    .resizable(true)
                    .min_width(280.0)
                    .default_width(320.0)
                    .max_width(450.0)
                    .show_separator_line(true)
                    .show_inside(ui, |ui| {
                    // Left column - Contacts management
                    ui.vertical(|ui| {
                        // Professional header with icon and styling
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("👥 Contacts")
                                    .heading()
                                    .strong()
                                    .color(super::theme::MutantColors::ACCENT_ORANGE)
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format!("{}", self.contacts.len()))
                                        .strong()
                                        .color(super::theme::MutantColors::ACCENT_BLUE)
                                );
                            });
                        });

                        ui.add_space(8.0);

                        // Professional add new contact section with better styling
                        egui::Frame::new()
                            .fill(super::theme::MutantColors::SURFACE)
                            .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_MEDIUM))
                            .inner_margin(egui::Margin::same(12))
                            .corner_radius(6.0)
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    // Section header
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new("➕ Add New Contact")
                                                .strong()
                                                .color(super::theme::MutantColors::ACCENT_GREEN)
                                        );
                                    });

                                    ui.add_space(6.0);

                                    // Pod Address input with better styling
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new("Pod Address")
                                                .size(11.0)
                                                .color(super::theme::MutantColors::TEXT_SECONDARY)
                                        );
                                        let _address_response = ui.add(
                                            egui::TextEdit::singleline(&mut self.new_contact_address)
                                                .hint_text("Enter pod address...")
                                                .desired_width(f32::INFINITY)
                                        );

                                        // Show validation feedback
                                        if !self.new_contact_address.is_empty() && self.new_contact_address.len() < 10 {
                                            ui.label(
                                                egui::RichText::new("⚠ Address seems too short")
                                                    .size(10.0)
                                                    .color(super::theme::MutantColors::WARNING)
                                            );
                                        }
                                    });

                                    ui.add_space(4.0);

                                    // Name input with better styling
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new("Display Name (optional)")
                                                .size(11.0)
                                                .color(super::theme::MutantColors::TEXT_SECONDARY)
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.new_contact_name)
                                                .hint_text("Enter friendly name...")
                                                .desired_width(f32::INFINITY)
                                        );
                                    });

                                    ui.add_space(8.0);

                                    // Action buttons with professional styling
                                    ui.horizontal(|ui| {
                                        let add_enabled = !self.new_contact_address.trim().is_empty();
                                        let add_button = ui.add_enabled(
                                            add_enabled,
                                            egui::Button::new(
                                                egui::RichText::new("✓ Add Contact")
                                                    .color(if add_enabled {
                                                        super::theme::MutantColors::TEXT_PRIMARY
                                                    } else {
                                                        super::theme::MutantColors::TEXT_DISABLED
                                                    })
                                            )
                                            .fill(if add_enabled {
                                                super::theme::MutantColors::ACCENT_GREEN
                                            } else {
                                                super::theme::MutantColors::SURFACE
                                            })
                                        );

                                        if add_button.clicked() {
                                            self.add_contact();
                                        }

                                        let clear_enabled = !self.new_contact_address.is_empty() || !self.new_contact_name.is_empty();
                                        let clear_button = ui.add_enabled(
                                            clear_enabled,
                                            egui::Button::new(
                                                egui::RichText::new("✕ Clear")
                                                    .color(super::theme::MutantColors::TEXT_SECONDARY)
                                            )
                                            .fill(super::theme::MutantColors::SURFACE)
                                            .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_MEDIUM))
                                        );

                                        if clear_button.clicked() {
                                            self.new_contact_address.clear();
                                            self.new_contact_name.clear();
                                        }
                                    });
                                });
                            });

                        ui.add_space(12.0);

                        // Professional contacts list section
                        ui.vertical(|ui| {
                            // Contacts list header with refresh button
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("📋 Contact List")
                                        .strong()
                                        .color(super::theme::MutantColors::ACCENT_BLUE)
                                );

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // Manual refresh button for debugging
                                    let refresh_button = ui.add_enabled(
                                        !self.is_loading_contacts,
                                        egui::Button::new(if self.is_loading_contacts { "Loading..." } else { "🔄" })
                                            .fill(super::theme::MutantColors::ACCENT_BLUE)
                                            .small()
                                    );

                                    if refresh_button.clicked() {
                                        log::info!("Manual contact list refresh requested");
                                        self.load_contacts();
                                    }

                                    refresh_button.on_hover_text("Refresh contact list");
                                });
                            });

                            ui.add_space(6.0);

                            // Show empty state or contact list
                            if self.contacts.is_empty() {
                                // Empty state with helpful message
                                egui::Frame::new()
                                    .fill(super::theme::MutantColors::BACKGROUND_LIGHT)
                                    .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_DARK))
                                    .inner_margin(egui::Margin::same(16))
                                    .corner_radius(4.0)
                                    .show(ui, |ui| {
                                        ui.vertical_centered(|ui| {
                                            ui.label(
                                                egui::RichText::new("👤")
                                                    .size(24.0)
                                                    .color(super::theme::MutantColors::TEXT_MUTED)
                                            );
                                            ui.label(
                                                egui::RichText::new("No contacts yet")
                                                    .strong()
                                                    .color(super::theme::MutantColors::TEXT_SECONDARY)
                                            );
                                            ui.label(
                                                egui::RichText::new("Add a contact above to start discovering content")
                                                    .size(11.0)
                                                    .color(super::theme::MutantColors::TEXT_MUTED)
                                            );
                                        });
                                    });
                            } else {
                                // Contact list with professional styling
                                ui.push_id("contacts_scroll", |ui| {
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false; 2])
                                        .max_height(300.0) // Limit height to prevent excessive scrolling
                                        .show(ui, |ui| {
                                            for (index, contact) in self.contacts.iter().enumerate() {
                                                // Professional contact item with better styling
                                                egui::Frame::new()
                                                    .fill(super::theme::MutantColors::BACKGROUND_LIGHT)
                                                    .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_DARK))
                                                    .inner_margin(egui::Margin::symmetric(10, 8))
                                                    .corner_radius(4.0)
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            // Contact status indicator
                                                            ui.label(
                                                                egui::RichText::new("●")
                                                                    .size(12.0)
                                                                    .color(super::theme::MutantColors::ACCENT_GREEN) // Online indicator
                                                            );

                                                            // Contact information
                                                            ui.vertical(|ui| {
                                                                // Contact name or fallback
                                                                let unnamed_contact = "Unnamed Contact".to_string();
                                                                let display_name = contact.name.as_ref()
                                                                    .unwrap_or(&unnamed_contact);
                                                                ui.label(
                                                                    egui::RichText::new(display_name)
                                                                        .strong()
                                                                        .size(12.0)
                                                                        .color(super::theme::MutantColors::ACCENT_ORANGE)
                                                                );

                                                                // Truncated address with tooltip
                                                                let truncated_address = if contact.pod_address.len() > 40 {
                                                                    format!("{}...{}",
                                                                        &contact.pod_address[..20],
                                                                        &contact.pod_address[contact.pod_address.len()-20..])
                                                                } else {
                                                                    contact.pod_address.clone()
                                                                };

                                                                let address_label = ui.label(
                                                                    egui::RichText::new(&truncated_address)
                                                                        .size(10.0)
                                                                        .color(super::theme::MutantColors::TEXT_MUTED)
                                                                        .family(egui::FontFamily::Monospace)
                                                                );

                                                                // Show full address on hover
                                                                address_label.on_hover_text(&contact.pod_address);

                                                                // Sync status
                                                                if let Some(last_synced) = &contact.last_synced {
                                                                    ui.horizontal(|ui| {
                                                                        ui.label(
                                                                            egui::RichText::new("🔄")
                                                                                .size(9.0)
                                                                                .color(super::theme::MutantColors::ACCENT_BLUE)
                                                                        );
                                                                        ui.label(
                                                                            egui::RichText::new(format!("Synced: {}", last_synced))
                                                                                .size(9.0)
                                                                                .color(super::theme::MutantColors::TEXT_MUTED)
                                                                        );
                                                                    });
                                                                } else {
                                                                    ui.horizontal(|ui| {
                                                                        ui.label(
                                                                            egui::RichText::new("⏳")
                                                                                .size(9.0)
                                                                                .color(super::theme::MutantColors::WARNING)
                                                                        );
                                                                        ui.label(
                                                                            egui::RichText::new("Never synced")
                                                                                .size(9.0)
                                                                                .color(super::theme::MutantColors::TEXT_MUTED)
                                                                        );
                                                                    });
                                                                }
                                                            });

                                                            // Action buttons
                                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                // Delete button with confirmation
                                                                let delete_button = ui.add(
                                                                    egui::Button::new(
                                                                        egui::RichText::new("🗑")
                                                                            .size(12.0)
                                                                            .color(super::theme::MutantColors::ERROR)
                                                                    )
                                                                    .fill(super::theme::MutantColors::SURFACE)
                                                                    .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::ERROR))
                                                                    .small()
                                                                );

                                                                if delete_button.clicked() {
                                                                    // TODO: Implement contact removal
                                                                    log::info!("Remove contact at index {}: {}", index, contact.pod_address);
                                                                }

                                                                delete_button.on_hover_text("Remove contact");

                                                                // Copy address button
                                                                let copy_button = ui.add(
                                                                    egui::Button::new(
                                                                        egui::RichText::new("📋")
                                                                            .size(12.0)
                                                                            .color(super::theme::MutantColors::ACCENT_BLUE)
                                                                    )
                                                                    .fill(super::theme::MutantColors::SURFACE)
                                                                    .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::ACCENT_BLUE))
                                                                    .small()
                                                                );

                                                                if copy_button.clicked() {
                                                                    ui.ctx().copy_text(contact.pod_address.clone());
                                                                    log::info!("Attempted to copy contact address to clipboard: {}", contact.pod_address);
                                                                }

                                                                copy_button.on_hover_text("Copy address");
                                                            });
                                                        });
                                                    });

                                                ui.add_space(4.0); // Space between contact items
                                            }
                                        });
                                });
                            }
                        });
                    });
                });
            }

            // Content discovery section (takes remaining space or full width when contacts are hidden)
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.vertical(|ui| {
                    // ui.horizontal(|ui| {
                    //     ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    //         // Refresh button
                    //         let refresh_button = ui.add_enabled(
                    //             !self.is_loading_content,
                    //             egui::Button::new(if self.is_loading_content { "Loading..." } else { "🔄 Refresh" })
                    //                 .fill(super::theme::MutantColors::ACCENT_BLUE)
                    //         );

                    //         if refresh_button.clicked() {
                    //             self.load_content_list();
                    //         }
                    //     });
                    // });

                    // Enhanced search section
                    egui::Frame::new()
                        .fill(super::theme::MutantColors::SURFACE)
                        .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_MEDIUM))
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .corner_radius(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("🔍 Search Content:")
                                        .strong()
                                        .color(super::theme::MutantColors::ACCENT_BLUE)
                                );

                                // Search input with better styling
                                let search_response = ui.add_sized(
                                    [ui.available_width() - 160.0, 24.0],
                                    egui::TextEdit::singleline(&mut self.search_query)
                                        .hint_text("Search for content across all pods...")
                                );

                                // Auto-search on enter
                                if search_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    self.search_content();
                                }

                                // Search and clear buttons with better styling
                                if ui.add(
                                    egui::Button::new("🔍 Search")
                                        .fill(super::theme::MutantColors::ACCENT_BLUE)
                                ).clicked() {
                                    self.search_content();
                                }

                                if ui.add(
                                    egui::Button::new("✖ Clear")
                                        .fill(super::theme::MutantColors::SURFACE_HOVER)
                                ).clicked() {
                                    self.search_query.clear();
                                    self.load_content_list();
                                }
                            });
                        });

                    ui.separator();

                    // Content discovery section with better organization
                    ui.add_space(8.0);

                    // Content summary header
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("📚 Discovered Content")
                                .heading()
                                .strong()
                                .color(super::theme::MutantColors::ACCENT_GREEN)
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!("{} items from {} pods",
                                    self.content_list.len(),
                                    self.pod_content.len()))
                                    .size(12.0)
                                    .color(super::theme::MutantColors::TEXT_SECONDARY)
                            );
                        });
                    });

                    ui.add_space(6.0);

                    // Enhanced content display with better styling
                    ui.push_id("content_scroll", |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                let mut download_address: Option<(String, String)> = None;

                                if self.pod_content.is_empty() {
                                    // Empty state with helpful message
                                    egui::Frame::new()
                                        .fill(super::theme::MutantColors::SURFACE)
                                        .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_MEDIUM))
                                        .inner_margin(egui::Margin::same(20))
                                        .corner_radius(8.0)
                                        .show(ui, |ui| {
                                            ui.vertical_centered(|ui| {
                                                ui.label(
                                                    egui::RichText::new("🔍 No content discovered yet")
                                                        .size(16.0)
                                                        .color(super::theme::MutantColors::TEXT_MUTED)
                                                );
                                                ui.add_space(8.0);
                                                ui.label(
                                                    egui::RichText::new("Add contacts and sync to discover content from the colony network")
                                                        .size(12.0)
                                                        .color(super::theme::MutantColors::TEXT_SECONDARY)
                                                );
                                            });
                                        });
                                } else {
                                    // Display pods with enhanced styling
                                    for (_pod_index, pod_content) in self.pod_content.iter_mut().enumerate() {
                                        egui::Frame::new()
                                            .fill(super::theme::MutantColors::SURFACE)
                                            .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_MEDIUM))
                                            .inner_margin(egui::Margin::symmetric(8, 6))
                                            .corner_radius(12.0)
                                            .show(ui, |ui| {
                                                // Enhanced pod header
                                                ui.horizontal(|ui| {
                                                    let expand_icon = if pod_content.is_expanded { "🔽" } else { "▶" };
                                                    if ui.add(
                                                        egui::Button::new(expand_icon)
                                                            .fill(super::theme::MutantColors::BACKGROUND_LIGHT)
                                                            .small()
                                                            .stroke(egui::Stroke::new(1.0, super::theme::MutantColors::BORDER_LIGHT))
                                                    ).clicked() {
                                                        pod_content.is_expanded = !pod_content.is_expanded;
                                                    }

                                                    ui.add_space(1.0);

                                                    // Pod icon and name
                                                    ui.label(
                                                        egui::RichText::new("🌐")
                                                            .size(11.0)
                                                            .color(super::theme::MutantColors::ACCENT_BLUE)
                                                    );

                                                    let pod_display_name = pod_content.pod_name.as_ref()
                                                        .unwrap_or(&pod_content.pod_address);
                                                    ui.label(
                                                        egui::RichText::new(pod_display_name)
                                                            .strong()
                                                            .size(11.0)
                                                            .color(super::theme::MutantColors::TEXT_PRIMARY)
                                                    );

                                                    // Content count badge
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        egui::Frame::new()
                                                            .fill(super::theme::MutantColors::ACCENT_ORANGE)
                                                            .inner_margin(egui::Margin::symmetric(8, 4))
                                                            .corner_radius(6.0)
                                                            .show(ui, |ui| {
                                                                ui.label(
                                                                    egui::RichText::new(format!("{}", pod_content.content_items.len()))
                                                                        .size(8.0)
                                                                        .strong()
                                                                        .color(super::theme::MutantColors::BACKGROUND_DARK)
                                                                );
                                                            });
                                                    });
                                                });

                                                // Show content items if expanded
                                                if pod_content.is_expanded {
                                                    ui.separator();
                                                    // Compact content items display
                                                    for content in &pod_content.content_items {
                                                        ui.horizontal(|ui| {
                                                            // Content type icon (compact)
                                                            let type_icon = match content.content_type.as_str() {
                                                                "video" => "🎥",
                                                                "audio" => "🎵",
                                                                "image" => "🖼️",
                                                                "document" => "📄",
                                                                _ => "📁"
                                                            };
                                                            ui.label(
                                                                egui::RichText::new(type_icon)
                                                                    .size(10.0)
                                                            );

                                                            ui.add_space(1.0);

                                                            // Title (truncated for compactness)
                                                            let truncated_title = if content.title.len() > 50 {
                                                                format!("{}...", &content.title[..47])
                                                            } else {
                                                                content.title.clone()
                                                            };
                                                            ui.label(
                                                                egui::RichText::new(&truncated_title)
                                                                    .size(10.0)
                                                                    .color(super::theme::MutantColors::TEXT_PRIMARY)
                                                            );

                                                            // Right-aligned metadata (compact)
                                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                // Compact download button
                                                                if ui.add(
                                                                    egui::Button::new("⬇")
                                                                        .fill(super::theme::MutantColors::ACCENT_BLUE)
                                                                        .stroke(egui::Stroke::NONE)
                                                                        .small()
                                                                ).clicked() {
                                                                    download_address = Some((content.address.clone(), content.title.clone()));
                                                                }

                                                                ui.add_space(1.0);

                                                                // Size (compact)
                                                                if let Some(size) = content.size {
                                                                    ui.label(
                                                                        egui::RichText::new(Self::format_file_size(size))
                                                                            .size(9.0)
                                                                            .color(super::theme::MutantColors::TEXT_MUTED)
                                                                    );
                                                                    ui.add_space(1.0);
                                                                }

                                                                // Date (compact)
                                                                if let Some(date) = &content.date_created {
                                                                    ui.label(
                                                                        egui::RichText::new(Self::format_date(date))
                                                                            .size(9.0)
                                                                            .color(super::theme::MutantColors::TEXT_MUTED)
                                                                    );
                                                                }
                                                            });
                                                        });
                                                        // Minimal spacing between items
                                                        ui.add_space(1.0);
                                                    }
                                                }
                                            });
                                        ui.add_space(1.0);
                                    }
                                }

                                // Handle download outside the loop to avoid borrowing issues
                                if let Some((address, filename)) = download_address {
                                    self.download_content(&address, &filename);
                                }
                            });
                    });
                });
            });
                }); // Close ScrollArea
            }); // Close allocate_ui_with_layout

        // Enhanced progress footer with better styling
        ui.add_space(8.0);

        // Progress section with frame
        egui::Frame::new()
            .fill(super::theme::MutantColors::SURFACE)
            .stroke(egui::Stroke::new(
                1.0,
                super::theme::MutantColors::BORDER_MEDIUM,
            ))
            .inner_margin(egui::Margin::symmetric(4, 2))
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Status icon
                    let (status_icon, status_color) = if self.operation_in_progress {
                        ("⏳", super::theme::MutantColors::ACCENT_ORANGE)
                    } else if self.current_operation_status.is_some() {
                        ("✅", super::theme::MutantColors::SUCCESS)
                    } else {
                        ("🟢", super::theme::MutantColors::ACCENT_GREEN)
                    };

                    ui.label(
                        egui::RichText::new(status_icon)
                            .size(14.0)
                            .color(status_color),
                    );

                    ui.add_space(8.0);

                    if self.operation_in_progress {
                        // Active progress display
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                // Progress bar with better styling
                                let progress_bar = egui::ProgressBar::new(self.current_progress)
                                    .fill(super::theme::MutantColors::ACCENT_ORANGE)
                                    .animate(true)
                                    .desired_width(content_width - 70.0)
                                    .desired_height(8.0);
                                ui.add(progress_bar);

                                // Progress percentage
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.0}%",
                                        self.current_progress * 100.0
                                    ))
                                    .strong()
                                    .color(super::theme::MutantColors::ACCENT_ORANGE)
                                    .size(12.0),
                                );
                            });

                            // Status message
                            if let Some(status) = &self.current_operation_status {
                                ui.label(
                                    egui::RichText::new(status)
                                        .color(super::theme::MutantColors::TEXT_PRIMARY)
                                        .size(11.0),
                                );
                            }
                        });
                    } else if let Some(status) = &self.current_operation_status {
                        // Completed operation status
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                // Completed progress bar
                                let progress_bar = egui::ProgressBar::new(1.0)
                                    .fill(super::theme::MutantColors::SUCCESS)
                                    .desired_width(content_width - 100.0)
                                    .desired_height(8.0);
                                ui.add(progress_bar);

                                ui.label(
                                    egui::RichText::new("Complete")
                                        .strong()
                                        .color(super::theme::MutantColors::SUCCESS)
                                        .size(12.0),
                                );
                            });

                            ui.label(
                                egui::RichText::new(status)
                                    .color(super::theme::MutantColors::TEXT_PRIMARY)
                                    .size(11.0),
                            );
                        });
                    } else {
                        // Ready state
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                // Ready progress bar
                                let progress_bar = egui::ProgressBar::new(0.0)
                                    .fill(super::theme::MutantColors::BORDER_MEDIUM)
                                    .desired_width(content_width - 80.0)
                                    .desired_height(8.0);
                                ui.add(progress_bar);

                                ui.label(
                                    egui::RichText::new("Ready")
                                        .color(super::theme::MutantColors::TEXT_SECONDARY)
                                        .size(12.0),
                                );
                            });

                            ui.label(
                                egui::RichText::new("Colony network ready for operations")
                                    .color(super::theme::MutantColors::TEXT_MUTED)
                                    .size(11.0),
                            );
                        });
                    }
                });
            });
    }
}

impl ColonyWindow {
    /// Check for and process colony progress events
    fn check_progress_events(&mut self) {
        if let Ok(mut events) = COLONY_PROGRESS_EVENTS.lock() {
            if !events.is_empty() {
                log::debug!("Processing {} colony progress events", events.len());
                // Process new events
                for event in events.drain(..) {
                    log::debug!("Processing colony event: {:?}", event.event);
                    self.progress_events.push(event.clone());

                    // Update current operation status based on the event
                    match &event.event {
                        mutant_protocol::ColonyEvent::InitializationStarted => {
                            self.current_operation_status =
                                Some("Initializing colony manager...".to_string());
                            self.operation_in_progress = true;
                            self.current_progress = 0.0;
                        }
                        mutant_protocol::ColonyEvent::InitializationCompleted => {
                            self.current_operation_status =
                                Some("Colony manager initialized".to_string());
                            self.operation_in_progress = false;
                            self.current_progress = 1.0;
                        }
                        mutant_protocol::ColonyEvent::AddContactStarted { pod_address } => {
                            self.current_operation_status =
                                Some(format!("Adding contact: {}", pod_address));
                            self.operation_in_progress = true;
                            self.current_progress = 0.0;
                        }
                        mutant_protocol::ColonyEvent::ContactVerificationStarted {
                            pod_address,
                        } => {
                            self.current_operation_status =
                                Some(format!("Verifying contact: {}", pod_address));
                            self.current_progress = 0.3;
                        }
                        mutant_protocol::ColonyEvent::ContactVerificationCompleted {
                            pod_address,
                            exists,
                        } => {
                            if *exists {
                                self.current_operation_status =
                                    Some(format!("Contact verified: {}", pod_address));
                            } else {
                                self.current_operation_status =
                                    Some(format!("Contact not found: {}", pod_address));
                            }
                            self.current_progress = 0.7;
                        }
                        mutant_protocol::ColonyEvent::AddContactCompleted { pod_address } => {
                            self.current_operation_status =
                                Some(format!("Contact added: {}", pod_address));
                            self.operation_in_progress = false;
                            self.current_progress = 1.0;
                            // Refresh contacts list after adding
                            self.should_load_contacts = true;
                            // Also refresh content list to show new content from the added contact
                            self.should_load_content = true;
                        }
                        mutant_protocol::ColonyEvent::SyncContactsStarted { total_contacts } => {
                            self.current_operation_status =
                                Some(format!("Syncing {} contacts...", total_contacts));
                            self.is_syncing = true;
                            self.operation_in_progress = true;
                            self.current_progress = 0.0;
                        }
                        mutant_protocol::ColonyEvent::ContactSyncStarted {
                            pod_address,
                            contact_index,
                            total_contacts,
                        } => {
                            self.current_operation_status = Some(format!(
                                "Syncing contact {} of {}: {}",
                                contact_index + 1,
                                total_contacts,
                                pod_address
                            ));
                            if *total_contacts > 0 {
                                self.current_progress =
                                    (*contact_index as f32) / (*total_contacts as f32);
                            }
                        }
                        mutant_protocol::ColonyEvent::ContactSyncCompleted {
                            pod_address,
                            contact_index,
                            total_contacts,
                        } => {
                            self.current_operation_status = Some(format!(
                                "Synced contact {} of {}: {}",
                                contact_index + 1,
                                total_contacts,
                                pod_address
                            ));
                            if *total_contacts > 0 {
                                self.current_progress =
                                    ((*contact_index + 1) as f32) / (*total_contacts as f32);
                            }
                        }
                        mutant_protocol::ColonyEvent::SyncContactsCompleted { synced_count } => {
                            self.current_operation_status =
                                Some(format!("Sync completed: {} contacts synced", synced_count));
                            self.is_syncing = false;
                            self.operation_in_progress = false;
                            self.current_progress = 1.0;
                            // Refresh content list after syncing
                            self.should_load_content = true;
                        }
                        mutant_protocol::ColonyEvent::IndexingStarted { user_key } => {
                            self.current_operation_status =
                                Some(format!("Indexing content: {}", user_key));
                            self.operation_in_progress = true;
                            self.current_progress = 0.0;
                        }
                        mutant_protocol::ColonyEvent::IndexingCompleted { user_key, success } => {
                            if *success {
                                self.current_operation_status =
                                    Some(format!("Content indexed: {}", user_key));
                            } else {
                                self.current_operation_status =
                                    Some(format!("Failed to index: {}", user_key));
                            }
                            self.operation_in_progress = false;
                            self.current_progress = 1.0;
                        }
                        mutant_protocol::ColonyEvent::SearchStarted { query_type } => {
                            self.current_operation_status =
                                Some(format!("Searching: {}", query_type));
                            self.operation_in_progress = true;
                            self.current_progress = 0.0;
                        }
                        mutant_protocol::ColonyEvent::SearchCompleted { results_count } => {
                            self.current_operation_status =
                                Some(format!("Search completed: {} results", results_count));
                            self.operation_in_progress = false;
                            self.current_progress = 1.0;
                        }
                        mutant_protocol::ColonyEvent::CacheRefreshStarted => {
                            self.current_operation_status = Some("Refreshing cache...".to_string());
                            self.operation_in_progress = true;
                            self.current_progress = 0.0;
                        }
                        mutant_protocol::ColonyEvent::CacheRefreshCompleted => {
                            self.current_operation_status = Some("Cache refreshed".to_string());
                            self.operation_in_progress = false;
                            self.current_progress = 1.0;
                        }
                        mutant_protocol::ColonyEvent::Progress {
                            operation,
                            message,
                            current,
                            total,
                        } => {
                            let progress_text =
                                if let (Some(current), Some(total)) = (current, total) {
                                    format!("{}: {} ({}/{})", operation, message, current, total)
                                } else {
                                    format!("{}: {}", operation, message)
                                };
                            self.current_operation_status = Some(progress_text);
                            self.operation_in_progress = true;

                            // Calculate progress if current and total are provided
                            if let (Some(current), Some(total)) = (current, total) {
                                if *total > 0 {
                                    self.current_progress = (*current as f32) / (*total as f32);
                                }
                            }
                        }
                        mutant_protocol::ColonyEvent::OperationCompleted { operation } => {
                            self.current_operation_status =
                                Some(format!("{} completed", operation));
                            self.operation_in_progress = false;
                            self.current_progress = 1.0;
                        }
                        mutant_protocol::ColonyEvent::OperationFailed { operation, error } => {
                            self.current_operation_status =
                                Some(format!("{} failed: {}", operation, error));
                            self.operation_in_progress = false;
                            self.current_progress = 0.0;
                            self.is_syncing = false; // Reset syncing state on any failure
                        }
                        // Additional detailed progress events for init operations
                        mutant_protocol::ColonyEvent::DataStoreInitStarted => {
                            self.current_operation_status =
                                Some("Initializing data store...".to_string());
                            self.current_progress = 0.1;
                        }
                        mutant_protocol::ColonyEvent::DataStoreInitCompleted => {
                            self.current_operation_status =
                                Some("Data store initialized".to_string());
                            self.current_progress = 0.2;
                        }
                        mutant_protocol::ColonyEvent::KeyStoreInitStarted => {
                            self.current_operation_status =
                                Some("Initializing key store...".to_string());
                            self.current_progress = 0.3;
                        }
                        mutant_protocol::ColonyEvent::KeyStoreInitCompleted => {
                            self.current_operation_status =
                                Some("Key store initialized".to_string());
                            self.current_progress = 0.4;
                        }
                        mutant_protocol::ColonyEvent::KeyDerivationStarted => {
                            self.current_operation_status = Some("Deriving keys...".to_string());
                            self.current_progress = 0.5;
                        }
                        mutant_protocol::ColonyEvent::KeyDerivationCompleted { pod_address } => {
                            self.current_operation_status =
                                Some(format!("Keys derived: {}", pod_address));
                            self.current_progress = 0.6;
                        }
                        mutant_protocol::ColonyEvent::GraphInitStarted => {
                            self.current_operation_status =
                                Some("Initializing graph database...".to_string());
                            self.current_progress = 0.7;
                        }
                        mutant_protocol::ColonyEvent::GraphInitCompleted => {
                            self.current_operation_status =
                                Some("Graph database initialized".to_string());
                            self.current_progress = 0.8;
                        }

                        // Additional detailed progress events for user pod operations
                        mutant_protocol::ColonyEvent::UserPodCheckStarted => {
                            self.current_operation_status =
                                Some("Checking user pod...".to_string());
                            self.current_progress = 0.0;
                        }
                        mutant_protocol::ColonyEvent::UserPodVerificationStarted {
                            pod_address,
                        } => {
                            self.current_operation_status =
                                Some(format!("Verifying pod: {}", pod_address));
                            self.current_progress = 0.2;
                        }
                        mutant_protocol::ColonyEvent::UserPodVerificationCompleted {
                            pod_address,
                            exists,
                        } => {
                            if *exists {
                                self.current_operation_status =
                                    Some(format!("Pod verified: {}", pod_address));
                            } else {
                                self.current_operation_status =
                                    Some(format!("Pod not found: {}", pod_address));
                            }
                            self.current_progress = 0.5;
                        }
                        mutant_protocol::ColonyEvent::UserPodCreationStarted => {
                            self.current_operation_status =
                                Some("Creating user pod...".to_string());
                            self.current_progress = 0.6;
                        }
                        mutant_protocol::ColonyEvent::UserPodCreationCompleted { pod_address } => {
                            self.current_operation_status =
                                Some(format!("Pod created: {}", pod_address));
                            self.current_progress = 0.9;
                        }
                        mutant_protocol::ColonyEvent::UserPodCheckCompleted => {
                            self.current_operation_status = Some("User pod ready".to_string());
                            self.current_progress = 1.0;
                        }

                        // Additional detailed progress events for contact operations
                        mutant_protocol::ColonyEvent::ContactRefAdditionStarted { pod_address } => {
                            self.current_operation_status =
                                Some(format!("Adding reference: {}", pod_address));
                            self.current_progress = 0.3;
                        }
                        mutant_protocol::ColonyEvent::ContactRefAdditionCompleted {
                            pod_address,
                        } => {
                            self.current_operation_status =
                                Some(format!("Reference added: {}", pod_address));
                            self.current_progress = 0.4;
                        }
                        mutant_protocol::ColonyEvent::ContactPodDownloadStarted { pod_address } => {
                            self.current_operation_status =
                                Some(format!("Downloading pod: {}", pod_address));
                            self.current_progress = 0.5;
                        }
                        mutant_protocol::ColonyEvent::ContactPodDownloadCompleted {
                            pod_address,
                        } => {
                            self.current_operation_status =
                                Some(format!("Pod downloaded: {}", pod_address));
                            self.current_progress = 0.7;
                        }
                        mutant_protocol::ColonyEvent::ContactPodUploadStarted => {
                            self.current_operation_status =
                                Some("Uploading updated pod...".to_string());
                            self.current_progress = 0.8;
                        }
                        mutant_protocol::ColonyEvent::ContactPodUploadCompleted => {
                            self.current_operation_status = Some("Pod uploaded".to_string());
                            self.current_progress = 0.9;
                        }

                        // Pod refresh events
                        mutant_protocol::ColonyEvent::PodRefreshStarted { total_pods } => {
                            self.current_operation_status =
                                Some(format!("Refreshing {} pods...", total_pods));
                            self.current_progress = 0.0;
                        }
                        mutant_protocol::ColonyEvent::PodRefreshCompleted { refreshed_pods } => {
                            self.current_operation_status =
                                Some(format!("Refreshed {} pods", refreshed_pods));
                            self.current_progress = 1.0;
                        }
                    }
                }

                // Keep only the last 50 events to prevent memory growth
                if self.progress_events.len() > 50 {
                    self.progress_events
                        .drain(0..self.progress_events.len() - 50);
                }
            }
        }
    }
    /// Add a new contact
    fn add_contact(&mut self) {
        if !self.new_contact_address.trim().is_empty() {
            let pod_address = self.new_contact_address.trim().to_string();
            let contact_name = if self.new_contact_name.trim().is_empty() {
                None
            } else {
                Some(self.new_contact_name.trim().to_string())
            };

            // Send to daemon - don't add to local list immediately
            // The daemon will send progress events and we'll refresh the contact list when complete
            let ctx = context();

            wasm_bindgen_futures::spawn_local(async move {
                match ctx.add_contact(&pod_address, contact_name).await {
                    Ok(_) => {
                        log::info!("Successfully added contact: {}", pod_address);
                    }
                    Err(e) => {
                        log::error!("Failed to add contact {}: {:?}", pod_address, e);
                    }
                }
            });

            // Clear input fields
            self.new_contact_address.clear();
            self.new_contact_name.clear();
        }
    }

    /// Sync all contacts to get latest content
    fn sync_all_contacts(&mut self) {
        if self.is_syncing {
            return;
        }

        self.is_syncing = true;

        let ctx = context();
        wasm_bindgen_futures::spawn_local(async move {
            match ctx.sync_contacts().await {
                Ok(response) => {
                    log::info!("Sync completed: {} contacts synced", response.synced_count);
                    // TODO: Update UI state
                }
                Err(e) => {
                    log::error!("Sync failed: {:?}", e);
                    // TODO: Update UI state with error
                }
            }
        });
    }

    /// Search for content
    fn search_content(&mut self) {
        if self.search_query.trim().is_empty() {
            self.refresh_content_list();
            return;
        }

        // Set loading state so the UI will pick up the results
        self.is_loading_content = true;

        let query = serde_json::json!({
            "type": "text",
            "text": self.search_query.trim(),
            "limit": 50
        });

        let ctx = context();
        wasm_bindgen_futures::spawn_local(async move {
            match ctx.search(query).await {
                Ok(response) => {
                    log::info!("Search completed: {:?}", response.results);

                    // Parse the SPARQL results into ContentItem structures
                    let content_items = Self::parse_content_response(response.results);

                    // Store the response in global state for the UI to pick up
                    if let Ok(mut responses) = CONTENT_LIST_RESPONSES.lock() {
                        responses.insert("content_list".to_string(), content_items);
                    }
                }
                Err(e) => {
                    log::error!("Search failed: {:?}", e);
                    // Store error state
                    if let Ok(mut responses) = CONTENT_LIST_RESPONSES.lock() {
                        responses.insert("content_list_error".to_string(), Vec::new());
                    }
                }
            }
        });
    }

    /// Load the content list from the daemon
    fn load_content_list(&mut self) {
        if self.is_loading_content {
            return;
        }

        self.is_loading_content = true;

        let ctx = context();
        wasm_bindgen_futures::spawn_local(async move {
            match ctx.list_content().await {
                Ok(response) => {
                    log::info!("Content list loaded: {:?}", response.content);

                    // Parse the SPARQL results into ContentItem structures
                    let content_items = Self::parse_content_response(response.content);

                    // Store the response in global state for the UI to pick up
                    if let Ok(mut responses) = CONTENT_LIST_RESPONSES.lock() {
                        responses.insert("content_list".to_string(), content_items);
                    }
                }
                Err(e) => {
                    log::error!("Failed to load content list: {:?}", e);
                    // Store error state
                    if let Ok(mut responses) = CONTENT_LIST_RESPONSES.lock() {
                        responses.insert("content_list_error".to_string(), Vec::new());
                    }
                }
            }
        });
    }

    /// Refresh the content list (alias for load_content_list for backward compatibility)
    fn refresh_content_list(&mut self) {
        self.load_content_list();
    }

    /// Parse search query results into ContentItem structures
    fn parse_content_response(content: serde_json::Value) -> Vec<ContentItem> {
        let mut content_items = Vec::new();

        // Handle error responses
        if let Some(error) = content.get("error") {
            log::warn!("Search returned error: {}", error);
            return content_items;
        }

        // Only handle SPARQL results format (sparql_results -> results -> bindings)
        if let Some(sparql_results) = content.get("sparql_results") {
            if let Some(results) = sparql_results.get("results") {
                if let Some(bindings) = results.get("bindings") {
                    if let Some(bindings_array) = bindings.as_array() {
                        log::info!("Found {} SPARQL bindings to parse", bindings_array.len());
                        content_items.extend(Self::parse_sparql_bindings(bindings_array));
                    }
                }
            }
        } else {
            log::warn!("Expected SPARQL results format but got: {:?}", content);
        }

        log::info!("Parsed {} content items from response", content_items.len());
        content_items
    }

    /// Parse multiple SPARQL bindings into ContentItem structures
    fn parse_sparql_bindings(bindings_array: &[serde_json::Value]) -> Vec<ContentItem> {
        use std::collections::HashMap;

        // Group bindings by subject to reconstruct complete objects
        let mut subjects: HashMap<String, HashMap<String, String>> = HashMap::new();

        for binding in bindings_array {
            if let (Some(subject), Some(predicate), Some(object)) = (
                binding
                    .get("subject")
                    .and_then(|s| s.get("value"))
                    .and_then(|v| v.as_str()),
                binding
                    .get("predicate")
                    .and_then(|p| p.get("value"))
                    .and_then(|v| v.as_str()),
                binding
                    .get("object")
                    .and_then(|o| o.get("value"))
                    .and_then(|v| v.as_str()),
            ) {
                subjects
                    .entry(subject.to_string())
                    .or_insert_with(HashMap::new)
                    .insert(predicate.to_string(), object.to_string());
            }
        }

        let mut content_items = Vec::new();

        for (subject_uri, properties) in subjects {
            // Debug: log all properties for this subject to understand what's available
            log::debug!(
                "Subject: {} has properties: {:?}",
                subject_uri,
                properties.keys().collect::<Vec<_>>()
            );

            // Filter out colony-specific metadata predicates first
            let has_colony_metadata = properties.keys().any(|key| {
                key.contains("colonylib/vocabulary")
                    || key.contains("pod_index")
                    || key == "ant://colonylib/vocabulary/0.1/predicate#date"
            });

            if has_colony_metadata {
                log::debug!("Skipping colony metadata subject: {}", subject_uri);
                continue;
            }

            // Check if this looks like file content (has description mentioning "File uploaded by")
            let has_file_description = properties
                .get("http://schema.org/description")
                .or_else(|| properties.get("schema:description"))
                .map(|desc| desc.contains("File uploaded by"))
                .unwrap_or(false);

            // For now, be more lenient - include anything that looks like file content
            if !has_file_description {
                log::debug!(
                    "Skipping non-file subject: {} (no file description)",
                    subject_uri
                );
                continue;
            }

            // Extract the address from the subject URI or url property
            let address = properties
                .get("http://schema.org/url")
                .or_else(|| properties.get("schema:url"))
                .cloned()
                .unwrap_or_else(|| {
                    if subject_uri.starts_with("ant://") {
                        subject_uri.replace("ant://", "")
                    } else {
                        subject_uri.clone()
                    }
                });

            // Extract Schema.org properties (try both formats: with and without schema: prefix)
            let title = properties
                .get("http://schema.org/name")
                .or_else(|| properties.get("schema:name"))
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    // If no name, create a title from the address
                    if address.len() > 16 {
                        format!(
                            "Content {}...{}",
                            &address[0..8],
                            &address[address.len() - 8..]
                        )
                    } else {
                        format!("Content {}", address)
                    }
                });

            let description = properties
                .get("http://schema.org/description")
                .or_else(|| properties.get("schema:description"))
                .map(|s| s.to_string());

            let content_type = properties
                .get("http://schema.org/type")
                .or_else(|| properties.get("@type"))
                .map(|s| Self::format_content_type(s))
                .unwrap_or_else(|| "Content".to_string());

            let source_contact = properties
                .get("http://schema.org/author")
                .or_else(|| properties.get("schema:author"))
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            let size = properties
                .get("http://schema.org/contentSize")
                .or_else(|| properties.get("schema:contentSize"))
                .and_then(|s| s.parse::<u64>().ok());

            let date_created = properties
                .get("http://schema.org/dateCreated")
                .or_else(|| properties.get("schema:dateCreated"))
                .map(|s| s.to_string());

            content_items.push(ContentItem {
                title,
                description,
                content_type,
                address,
                source_contact,
                size,
                date_created,
            });
        }

        log::info!(
            "Converted {} subjects into content items",
            content_items.len()
        );
        content_items
    }

    /// Format content type for display
    fn format_content_type(content_type: &str) -> String {
        // Convert Schema.org URIs to readable format
        if content_type.starts_with("http://schema.org/") {
            content_type.replace("http://schema.org/", "")
        } else if content_type.starts_with("https://schema.org/") {
            content_type.replace("https://schema.org/", "")
        } else {
            content_type.to_string()
        }
    }

    /// Download content by address and open in new viewer tab
    fn download_content(&self, address: &str, filename: &str) {
        log::info!("Downloading content from address: {} with filename: {}", address, filename);

        // Create a KeyDetails for the public address using the filename as the key
        let key_details = mutant_protocol::KeyDetails {
            key: filename.to_string(),
            total_size: 0,     // Unknown size for public content
            pad_count: 0,      // Unknown pad count
            confirmed_pads: 0, // Unknown confirmed pads
            is_public: true,
            public_address: Some(address.to_string()),
        };

        // Defer the tab addition to avoid deadlock since we're running inside the fs window
        let address_clone = address.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            // Now safely add the tab to the main fs window's internal dock system
            if let Some(fs_window_ref) = crate::app::fs::global::get_main_fs_window() {
                if let Ok(mut fs_window) = fs_window_ref.try_write() {
                    // Use the fs window's existing add_file_tab method which handles everything
                    fs_window.add_file_tab(key_details);

                    log::info!(
                        "Colony: Successfully requested file viewer tab addition to fs window for: {}",
                        address_clone
                    );
                } else {
                    log::warn!("Colony: Could not acquire write lock on fs window (may be busy)");
                }
            } else {
                log::warn!(
                    "Colony: Main FsWindow reference not available for adding file viewer tab"
                );
            }
        });
    }

    /// Format file size for display
    fn format_file_size(size: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size_f = size as f64;
        let mut unit_index = 0;

        while size_f >= 1024.0 && unit_index < UNITS.len() - 1 {
            size_f /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} {}", size, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size_f, UNITS[unit_index])
        }
    }

    /// Format date for display
    fn format_date(date_str: &str) -> String {
        // Simple date formatting - extract date and time from ISO 8601 format
        // Expected format: "2024-01-15T10:30:45.123Z" or similar
        if let Some(t_pos) = date_str.find('T') {
            let date_part = &date_str[..t_pos];
            if let Some(colon_pos) = date_str[t_pos..].find(':') {
                let time_part = &date_str[t_pos + 1..t_pos + colon_pos + 3]; // Get HH:MM
                format!("{} {}", date_part, time_part)
            } else {
                date_part.to_string()
            }
        } else {
            // Fallback to showing the raw string if parsing fails
            date_str.to_string()
        }
    }

    /// Load contacts from the daemon
    fn load_contacts(&mut self) {
        if self.is_loading_contacts {
            log::debug!("Contact loading already in progress, skipping");
            return;
        }

        log::info!("Starting contact list load from daemon");
        self.is_loading_contacts = true;

        let ctx = context();
        wasm_bindgen_futures::spawn_local(async move {
            log::debug!("Sending list_contacts request to daemon");
            match ctx.list_contacts().await {
                Ok(response) => {
                    log::info!(
                        "Contacts loaded successfully: {} contacts",
                        response.contacts.len()
                    );
                    log::debug!("Contact addresses: {:?}", response.contacts);

                    // Store the contacts in global state for the UI to pick up
                    if let Ok(mut responses) = CONTACT_LIST_RESPONSES.lock() {
                        responses.insert("contact_list".to_string(), response.contacts);
                        log::debug!("Stored contacts in global state for UI pickup");
                    } else {
                        log::error!("Failed to acquire lock on CONTACT_LIST_RESPONSES");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load contacts: {:?}", e);
                    // Store error state
                    if let Ok(mut responses) = CONTACT_LIST_RESPONSES.lock() {
                        responses.insert("contact_list_error".to_string(), Vec::new());
                    }
                }
            }
        });
    }

    /// Load the user's own contact information
    fn load_user_contact_info(&mut self) {
        if self.is_loading_user_contact {
            return;
        }

        self.is_loading_user_contact = true;
        self.user_contact_info = None;

        let ctx = context();
        wasm_bindgen_futures::spawn_local(async move {
            match ctx.get_user_contact().await {
                Ok(response) => {
                    log::info!(
                        "Got user contact info: address={}, type={}, display_name={:?}",
                        response.contact_address,
                        response.contact_type,
                        response.display_name
                    );

                    // Store the response in global state for the UI to pick up
                    let user_info = UserContactInfo {
                        contact_address: response.contact_address,
                        contact_type: response.contact_type,
                        display_name: response.display_name,
                    };

                    if let Ok(mut responses) = USER_CONTACT_RESPONSES.lock() {
                        responses.insert("user_contact".to_string(), user_info);
                    }
                }
                Err(e) => {
                    log::error!("Failed to get user contact info: {:?}", e);
                    // Clear loading state on error by storing an empty response
                    if let Ok(mut responses) = USER_CONTACT_RESPONSES.lock() {
                        responses.insert(
                            "user_contact_error".to_string(),
                            UserContactInfo {
                                contact_address: "Error loading contact info".to_string(),
                                contact_type: "error".to_string(),
                                display_name: None,
                            },
                        );
                    }
                }
            }
        });
    }

    /// Organize content items by their source pods
    fn organize_content_by_pods(&mut self) {
        use std::collections::HashMap;

        // Get the user's own pod address to filter it out
        let user_pod_address = self
            .user_contact_info
            .as_ref()
            .map(|info| &info.contact_address);

        // Group content by source_contact (pod address), filtering out user's own content
        let mut pod_groups: HashMap<String, Vec<ContentItem>> = HashMap::new();

        for content_item in &self.content_list {
            // Skip content from the user's own pod
            if let Some(user_pod) = user_pod_address {
                if content_item.source_contact == *user_pod {
                    log::debug!("Filtering out user's own content: {}", content_item.title);
                    continue;
                }
            }

            pod_groups
                .entry(content_item.source_contact.clone())
                .or_insert_with(Vec::new)
                .push(content_item.clone());
        }

        // Convert to PodContent structures
        self.pod_content = pod_groups
            .into_iter()
            .map(|(pod_address, content_items)| {
                // Try to find a friendly name for this pod from our contacts
                let pod_name = self
                    .contacts
                    .iter()
                    .find(|contact| contact.pod_address == pod_address)
                    .and_then(|contact| contact.name.clone());

                PodContent {
                    pod_address,
                    pod_name,
                    content_items,
                    is_expanded: true, // Start with all pods expanded
                }
            })
            .collect();

        // Sort pods by name/address for consistent display
        self.pod_content.sort_by(|a, b| {
            let a_display = a.pod_name.as_ref().unwrap_or(&a.pod_address);
            let b_display = b.pod_name.as_ref().unwrap_or(&b.pod_address);
            a_display.cmp(b_display)
        });

        let filtered_count = self.content_list.len()
            - self
                .pod_content
                .iter()
                .map(|p| p.content_items.len())
                .sum::<usize>();
        log::info!(
            "Organized {} content items into {} pods (filtered out {} user's own items)",
            self.content_list.len(),
            self.pod_content.len(),
            filtered_count
        );
    }
}

/// Check if any colony operation is currently in progress
pub fn is_colony_operation_in_progress() -> bool {
    // During onboarding, always show the modal if we're in colony initialization step
    // This covers the entire colony initialization process, including initial pod creation
    if crate::app::onboarding::is_colony_initializing() {
        log::debug!("is_colony_operation_in_progress: colony initialization in progress, returning true");
        return true;
    }

    // Check if there are recent colony initialization events
    if let Ok(events) = COLONY_PROGRESS_EVENTS.try_lock() {
        log::debug!("is_colony_operation_in_progress: checking {} events", events.len());
        // Look for recent initialization events
        for event in events.iter().rev().take(10) { // Check last 10 events
            log::debug!("is_colony_operation_in_progress: checking event: {:?}", event.event);
            match &event.event {
                mutant_protocol::ColonyEvent::InitializationStarted => {
                    log::debug!("is_colony_operation_in_progress: found InitializationStarted, returning true");
                    return true;
                }
                mutant_protocol::ColonyEvent::InitializationCompleted => {
                    log::debug!("is_colony_operation_in_progress: found InitializationCompleted, returning false");
                    return false;
                }
                mutant_protocol::ColonyEvent::DataStoreInitStarted |
                mutant_protocol::ColonyEvent::KeyStoreInitStarted |
                mutant_protocol::ColonyEvent::KeyDerivationStarted |
                mutant_protocol::ColonyEvent::GraphInitStarted |
                mutant_protocol::ColonyEvent::UserPodCheckStarted |
                mutant_protocol::ColonyEvent::UserPodCreationStarted => return true,
                _ => continue,
            }
        }
    }
    false
}

/// Get current colony operation progress information
pub fn get_colony_operation_progress() -> (f32, Option<String>) {
    if let Ok(events) = COLONY_PROGRESS_EVENTS.try_lock() {
        // Look for the most recent progress information
        for event in events.iter().rev() {
            match &event.event {
                mutant_protocol::ColonyEvent::InitializationStarted => {
                    return (0.0, Some("Colony initialization started...".to_string()));
                }
                mutant_protocol::ColonyEvent::DataStoreInitStarted => {
                    return (0.1, Some("Initializing data store...".to_string()));
                }
                mutant_protocol::ColonyEvent::DataStoreInitCompleted => {
                    return (0.2, Some("Data store initialized".to_string()));
                }
                mutant_protocol::ColonyEvent::KeyStoreInitStarted => {
                    return (0.3, Some("Initializing key store...".to_string()));
                }
                mutant_protocol::ColonyEvent::KeyStoreInitCompleted => {
                    return (0.4, Some("Key store initialized".to_string()));
                }
                mutant_protocol::ColonyEvent::KeyDerivationStarted => {
                    return (0.5, Some("Deriving colony keys...".to_string()));
                }
                mutant_protocol::ColonyEvent::KeyDerivationCompleted { pod_address } => {
                    return (0.6, Some(format!("Keys derived for pod: {}", pod_address)));
                }
                mutant_protocol::ColonyEvent::GraphInitStarted => {
                    return (0.7, Some("Initializing graph database...".to_string()));
                }
                mutant_protocol::ColonyEvent::GraphInitCompleted => {
                    return (0.8, Some("Graph database initialized".to_string()));
                }
                mutant_protocol::ColonyEvent::UserPodCheckStarted => {
                    return (0.85, Some("Checking user pod...".to_string()));
                }
                mutant_protocol::ColonyEvent::UserPodCreationStarted => {
                    return (0.9, Some("Creating user pod...".to_string()));
                }
                mutant_protocol::ColonyEvent::UserPodCreationCompleted { pod_address } => {
                    return (0.95, Some(format!("User pod created: {}", pod_address)));
                }
                mutant_protocol::ColonyEvent::UserPodCheckCompleted => {
                    return (0.98, Some("User pod check completed".to_string()));
                }
                mutant_protocol::ColonyEvent::InitializationCompleted => {
                    return (1.0, Some("Colony initialization completed!".to_string()));
                }
                mutant_protocol::ColonyEvent::Progress { operation, message, current, total } => {
                    let progress = if let (Some(current), Some(total)) = (current, total) {
                        if *total > 0 {
                            (*current as f32) / (*total as f32)
                        } else {
                            0.5
                        }
                    } else {
                        0.5
                    };
                    return (progress, Some(format!("{}: {}", operation, message)));
                }
                _ => continue,
            }
        }
    }
    (0.0, None)
}

/// Add a colony progress event to the global state for UI updates
pub fn add_colony_progress_event(
    event: mutant_protocol::ColonyEvent,
    operation_id: Option<String>,
) {
    log::debug!("Adding colony progress event: {:?}", event);

    let progress_event = ColonyProgressEvent {
        event: event.clone(),
        operation_id: operation_id.clone(),
    };

    // Use try_lock to avoid blocking and potential deadlocks
    // Don't use catch_unwind as it's not properly supported in WebAssembly
    match COLONY_PROGRESS_EVENTS.try_lock() {
        Ok(mut events) => {
            events.push(progress_event);
            log::debug!(
                "Colony progress events queue now has {} events",
                events.len()
            );

            // Keep only the last 100 events to prevent memory growth
            let len = events.len();
            if len > 100 {
                events.drain(0..len - 100);
            }
        }
        Err(std::sync::TryLockError::WouldBlock) => {
            log::debug!("COLONY_PROGRESS_EVENTS mutex is busy, skipping event: {:?}", event);
            // Don't panic, just skip this event
        }
        Err(std::sync::TryLockError::Poisoned(e)) => {
            log::error!("COLONY_PROGRESS_EVENTS mutex is poisoned: {}", e);
            // Try to recover the mutex and continue
            match e.into_inner() {
                mut events => {
                    events.push(ColonyProgressEvent {
                        event,
                        operation_id,
                    });
                    log::debug!("Recovered from poisoned mutex and added event");
                }
            }
        }
    }
}
