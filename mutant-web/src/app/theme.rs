use eframe::egui::{self, Color32, Stroke};

/// MutAnt color palette - professional, techy, somber with bright accents
pub struct MutantColors;

impl MutantColors {
    // Base colors - dark/somber theme
    pub const BACKGROUND_DARK: Color32 = Color32::from_rgb(12, 12, 15);        // Very dark background
    pub const BACKGROUND_MEDIUM: Color32 = Color32::from_rgb(18, 18, 22);      // Medium dark
    pub const BACKGROUND_LIGHT: Color32 = Color32::from_rgb(25, 25, 30);       // Lighter dark
    pub const SURFACE: Color32 = Color32::from_rgb(30, 30, 36);                // Surface elements
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(35, 35, 42);          // Hovered surfaces
    
    // Border and stroke colors
    pub const BORDER_DARK: Color32 = Color32::from_rgb(40, 40, 48);            // Dark borders
    pub const BORDER_MEDIUM: Color32 = Color32::from_rgb(55, 55, 65);          // Medium borders
    pub const BORDER_LIGHT: Color32 = Color32::from_rgb(70, 70, 80);           // Light borders
    
    // Text colors
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(240, 240, 245);        // Primary text
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(180, 180, 190);      // Secondary text
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(120, 120, 130);          // Muted text
    pub const TEXT_DISABLED: Color32 = Color32::from_rgb(80, 80, 90);          // Disabled text
    
    // Bright accent colors - fluorescent/neon
    pub const ACCENT_ORANGE: Color32 = Color32::from_rgb(255, 140, 0);         // Bright orange
    pub const ACCENT_BLUE: Color32 = Color32::from_rgb(0, 150, 255);           // Bright blue
    pub const ACCENT_GREEN: Color32 = Color32::from_rgb(50, 255, 150);         // Bright green
    pub const ACCENT_PURPLE: Color32 = Color32::from_rgb(180, 100, 255);       // Bright purple
    pub const ACCENT_CYAN: Color32 = Color32::from_rgb(0, 255, 200);           // Bright cyan
    
    // Status colors
    pub const SUCCESS: Color32 = Color32::from_rgb(50, 255, 150);              // Success green
    pub const WARNING: Color32 = Color32::from_rgb(255, 200, 0);               // Warning yellow
    pub const ERROR: Color32 = Color32::from_rgb(255, 80, 80);                 // Error red
    pub const INFO: Color32 = Color32::from_rgb(0, 150, 255);                  // Info blue
    
    // Special colors
    pub const SELECTION: Color32 = Color32::from_rgba_premultiplied(45, 45, 55, 120);  // Dark grey selection overlay
    pub const HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(255, 140, 0, 40);  // Highlight overlay
}

/// Apply the MutAnt theme to the egui context
pub fn apply_mutant_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = style.visuals.clone();

    // === BACKGROUND COLORS ===
    visuals.window_fill = MutantColors::BACKGROUND_MEDIUM;
    visuals.panel_fill = MutantColors::BACKGROUND_DARK;
    visuals.faint_bg_color = MutantColors::BACKGROUND_LIGHT;
    visuals.extreme_bg_color = MutantColors::BACKGROUND_DARK;

    // === WINDOW STYLING ===
    visuals.window_stroke = Stroke::new(1.5, MutantColors::BORDER_MEDIUM);

    // === TEXT COLORS ===
    visuals.override_text_color = Some(MutantColors::TEXT_PRIMARY);

    // === SELECTION COLORS ===
    visuals.selection.bg_fill = MutantColors::SELECTION;
    visuals.selection.stroke = Stroke::new(1.0, MutantColors::ACCENT_BLUE);

    // === HYPERLINK COLORS ===
    visuals.hyperlink_color = MutantColors::ACCENT_BLUE;

    // === WIDGET STYLING ===

    // Inactive widgets (default state)
    visuals.widgets.noninteractive.bg_fill = MutantColors::SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, MutantColors::BORDER_DARK);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MutantColors::TEXT_SECONDARY);

    // Inactive widgets (hovered)
    visuals.widgets.inactive.bg_fill = MutantColors::SURFACE_HOVER;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, MutantColors::BORDER_MEDIUM);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, MutantColors::TEXT_PRIMARY);

    // Hovered widgets
    visuals.widgets.hovered.bg_fill = MutantColors::SURFACE_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, MutantColors::ACCENT_ORANGE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, MutantColors::TEXT_PRIMARY);

    // Active widgets (pressed/selected)
    visuals.widgets.active.bg_fill = MutantColors::ACCENT_ORANGE;
    visuals.widgets.active.bg_stroke = Stroke::new(2.0, MutantColors::ACCENT_ORANGE);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, MutantColors::BACKGROUND_DARK);

    // Open widgets (like combo boxes)
    visuals.widgets.open.bg_fill = MutantColors::SURFACE;
    visuals.widgets.open.bg_stroke = Stroke::new(1.5, MutantColors::ACCENT_BLUE);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, MutantColors::TEXT_PRIMARY);

    // === TAB STYLING ===
    // Style tab headers to match the dark, professional theme
    visuals.widgets.noninteractive.weak_bg_fill = MutantColors::BACKGROUND_MEDIUM; // Inactive tab background
    visuals.widgets.inactive.weak_bg_fill = MutantColors::SURFACE; // Inactive tab background (hovered)
    visuals.widgets.hovered.weak_bg_fill = MutantColors::SURFACE_HOVER; // Hovered tab background
    visuals.widgets.active.weak_bg_fill = MutantColors::BACKGROUND_LIGHT; // Active tab background

    // Tab text colors - these will be applied by egui_dock
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MutantColors::TEXT_MUTED); // Inactive tab text (dimmed white)
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, MutantColors::TEXT_MUTED); // Inactive tab text (dimmed white)
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, MutantColors::TEXT_PRIMARY); // Hovered tab text
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, MutantColors::ACCENT_ORANGE); // Active tab text (orange)

    // Close button styling (red)
    visuals.error_fg_color = MutantColors::ERROR; // Close button color

    // Tab bar background
    visuals.window_fill = MutantColors::BACKGROUND_MEDIUM;
    visuals.panel_fill = MutantColors::BACKGROUND_MEDIUM;

    // === RESIZE HANDLE ===
    visuals.resize_corner_size = 12.0;

    // === INDENT AND SPACING ===
    style.spacing.indent = 20.0;
    style.spacing.item_spacing = [8.0, 6.0].into();
    style.spacing.button_padding = [12.0, 8.0].into();
    style.spacing.indent_ends_with_horizontal_line = true;

    // === TEXT STYLES ===
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(18.0, egui::FontFamily::Proportional), // Smaller heading
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(13.0, egui::FontFamily::Proportional), // Smaller body text
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(13.0, egui::FontFamily::Proportional), // Smaller button text
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(11.0, egui::FontFamily::Proportional), // Smaller small text
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(12.0, egui::FontFamily::Monospace), // Smaller monospace
    );

    // Apply the style
    style.visuals = visuals;
    ctx.set_style(style);
}

/// Create a styled button with MutAnt theming
pub fn styled_button(text: &str, accent_color: Color32) -> egui::Button {
    egui::Button::new(egui::RichText::new(text).color(MutantColors::TEXT_PRIMARY))
        .fill(MutantColors::SURFACE)
        .stroke(Stroke::new(1.0, accent_color))
}

/// Create a primary action button (orange accent)
pub fn primary_button(text: &str) -> egui::Button {
    styled_button(text, MutantColors::ACCENT_ORANGE)
}

/// Create a secondary action button (blue accent)
pub fn secondary_button(text: &str) -> egui::Button {
    styled_button(text, MutantColors::ACCENT_BLUE)
}

/// Create a success button (green accent)
pub fn success_button(text: &str) -> egui::Button {
    styled_button(text, MutantColors::SUCCESS)
}

/// Create a danger button (red accent)
pub fn danger_button(text: &str) -> egui::Button {
    styled_button(text, MutantColors::ERROR)
}

/// Create a styled progress bar with MutAnt theming
pub fn styled_progress_bar(progress: f32, accent_color: Color32) -> egui::ProgressBar {
    egui::ProgressBar::new(progress)
        .fill(accent_color)
        .animate(true)
}

/// Create a primary progress bar (orange)
pub fn primary_progress_bar(progress: f32) -> egui::ProgressBar {
    styled_progress_bar(progress, MutantColors::ACCENT_ORANGE)
}

/// Create a success progress bar (green)
pub fn success_progress_bar(progress: f32) -> egui::ProgressBar {
    styled_progress_bar(progress, MutantColors::SUCCESS)
}

/// Create a info progress bar (blue)
pub fn info_progress_bar(progress: f32) -> egui::ProgressBar {
    styled_progress_bar(progress, MutantColors::ACCENT_BLUE)
}

/// Create a styled section group with MutAnt theming
pub fn styled_section_group() -> egui::Frame {
    egui::Frame::group(&egui::Style::default())
        .fill(MutantColors::SURFACE)
        .stroke(egui::Stroke::new(1.0, MutantColors::BORDER_MEDIUM))
        .inner_margin(egui::Margin::same(12))
        .outer_margin(egui::Margin::same(4))
}

/// Create a prominent info section frame
pub fn info_section_frame() -> egui::Frame {
    egui::Frame::group(&egui::Style::default())
        .fill(MutantColors::BACKGROUND_LIGHT)
        .stroke(egui::Stroke::new(1.5, MutantColors::ACCENT_BLUE))
        .inner_margin(egui::Margin::same(16))
        .outer_margin(egui::Margin::same(6))
}

/// Create a warning section frame
pub fn warning_section_frame() -> egui::Frame {
    egui::Frame::group(&egui::Style::default())
        .fill(MutantColors::BACKGROUND_LIGHT)
        .stroke(egui::Stroke::new(1.5, MutantColors::WARNING))
        .inner_margin(egui::Margin::same(16))
        .outer_margin(egui::Margin::same(6))
}

/// Create a section header with consistent styling
pub fn section_header(text: &str, icon: &str, color: Color32) -> egui::RichText {
    egui::RichText::new(format!("{} {}", icon, text))
        .size(16.0)
        .strong()
        .color(color)
}

/// Create a file info display with icon and details
pub fn file_info_text(filename: &str, size: u64, file_type: &str) -> String {
    let size_str = if size > 1024 * 1024 * 1024 {
        format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if size > 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else if size > 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{} bytes", size)
    };

    format!("{}\n{} • {}", filename, size_str, file_type)
}
