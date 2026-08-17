//! The choices each game setting offers, and the actions a key can be
//! bound to. Values are what Sunrise stores; labels are what Destiny 2
//! calls them.

pub(super) const BUTTON_LAYOUTS: &[(u64, &str)] = &[
    (0, "Default"),
    (1, "Green Thumb"),
    (2, "Puppeteer"),
    (3, "Mirror"),
    (5, "Jumper"),
    (6, "Cold Shoulder"),
    (9, "Custom"),
];

pub(super) const STICK_LAYOUTS: &[(u64, &str)] = &[
    (0, "Default"),
    (1, "Southpaw"),
    (2, "Legacy"),
    (3, "Legacy Southpaw"),
];
pub(super) const DOUBLE_PRESS_DELAYS: &[(u64, &str)] = &[
    (0, "1 — 167 ms (Default)"),
    (1, "2 — 212 ms"),
    (2, "3 — 302 ms"),
    (3, "4 — 347 ms"),
    (4, "5 — 392 ms"),
];
pub(super) const VOICE_OUTPUT_MODES: &[(u64, &str)] = &[
    (0, "Blended"),
    (1, "Headset Only (Default)"),
    (2, "Speakers Only"),
];
pub(super) const TEAM_VOICE_MODES: &[(u64, &str)] = &[
    (0, "Manually Opt-in (Default)"),
    (1, "Automatic Opt-in When Solo"),
];
pub(super) const PROXIMITY_VOICE_OUTPUTS: &[(u64, &str)] = &[(0, "Speakers (Default)"), (1, "Headset Only")];
pub(super) const HDR_MODES: &[(u64, &str)] = &[(0, "Off (Default)"), (1, "On")];
pub(super) const SUBTITLE_MODES: &[(u64, &str)] = &[(0, "Language-Based (Default)"), (1, "On"), (2, "Off")];
pub(super) const COLORBLIND_MODES: &[(u64, &str)] = &[
    (0, "Off (Default)"),
    (1, "Deuteranopia (Red-Green)"),
    (2, "Protanopia (Red-Green)"),
    (3, "Tritanopia (Yellow-Blue)"),
];
pub(super) const HELMET_MODES: &[(u64, &str)] = &[(0, "Off in Non-Combat Zones"), (1, "Always On")];
pub(super) const HUD_OPACITY: &[(u64, &str)] = &[(0, "Off"), (1, "Low"), (2, "High"), (3, "Full (Default)")];
pub(super) const BACKGROUND_OPACITY: &[(u64, &str)] = &[
    (0, "Lowest"),
    (1, "Low"),
    (2, "Medium (Default)"),
    (3, "High"),
    (4, "Highest"),
];
pub(super) const RETICLE_LOCATIONS: &[(u64, &str)] = &[(0, "PC Default"), (1, "Console Default")];
pub(super) const TEXT_CHAT_MODES: &[(u64, &str)] = &[
    (0, "Off"),
    (1, "On (No Notifications)"),
    (2, "On (No Audio)"),
    (3, "On (Default)"),
];
pub(super) const WHISPER_CHAT_MODES: &[(u64, &str)] = &[(0, "On (Default)"), (1, "Off")];
pub(super) const MANUAL_AUTOMATIC: &[(u64, &str)] = &[(0, "Manual"), (1, "Automatic")];
pub(super) const AUTO_HIDE_MODES: &[(u64, &str)] = &[(0, "Off"), (1, "On")];

/// One editable game setting: where it lives in the file, what the page calls
/// it, and what it may hold. The widget that edits it and the check that
/// validates it are both built from this, so a setting's domain is described
/// once and cannot drift between the two.
pub(super) struct Setting {
    pub(super) key: &'static str,
    pub(super) label: &'static str,
    pub(super) domain: Domain,
}

/// What a setting may hold.
pub(super) enum Domain {
    /// True or false.
    Flag,
    /// One of a named set, shown as a combo box.
    Choice(&'static [(u64, &'static str)]),
    Range { minimum: u64, maximum: u64 },
    /// A range the game numbers from one but stores from zero.
    Offset {
        minimum: u64,
        maximum: u64,
        display_offset: u64,
    },
    Decimal {
        minimum: f64,
        maximum: f64,
        step: f64,
    },
    /// A value Sunrise requires to stay as it is. Shown, never editable.
    Exact(u64),
    ExactDecimal(f32),
}

/// One object of `state.account.settings`, and the page that edits it.
pub(super) struct SettingGroup {
    pub(super) name: &'static str,
    pub(super) heading: &'static str,
    pub(super) description: &'static str,
    pub(super) settings: &'static [Setting],
}

/// Every group, in the order the tabs present them.
pub(super) const GROUPS: &[&SettingGroup] = &[&CONTROLS, &AUDIO, &DISPLAY, &INTERFACE, &SOCIAL];

pub(super) const CONTROLS: SettingGroup = SettingGroup {
    name: "controls",
    heading: "Controls",
    description: "Controller and mouse behavior.",
    settings: &[
        Setting { key: "button_layout", label: "Button layout", domain: Domain::Choice(BUTTON_LAYOUTS) },
        Setting { key: "movement_mode", label: "Stick layout", domain: Domain::Choice(STICK_LAYOUTS) },
        Setting { key: "controller_look_sensitivity", label: "Controller look sensitivity", domain: Domain::Offset { minimum: 0, maximum: 9, display_offset: 1 } },
        Setting { key: "controller_invert_vertical", label: "Invert controller vertical look", domain: Domain::Flag },
        Setting { key: "controller_auto_look_centering", label: "Controller auto-look centering", domain: Domain::Flag },
        Setting { key: "controller_vibration", label: "Controller vibration", domain: Domain::Flag },
        Setting { key: "controller_swap_shoulders", label: "Swap controller shoulder buttons", domain: Domain::Flag },
        Setting { key: "controller_invert_horizontal", label: "Invert controller horizontal look", domain: Domain::Flag },
        Setting { key: "mouse_look_sensitivity", label: "Mouse look sensitivity", domain: Domain::Range { minimum: 1, maximum: 100 } },
        Setting { key: "mouse_invert_vertical", label: "Invert mouse vertical look", domain: Domain::Flag },
        Setting { key: "mouse_invert_horizontal", label: "Invert mouse horizontal look", domain: Domain::Flag },
        Setting { key: "unidentified_toggle", label: "Unidentified control toggle", domain: Domain::Flag },
        Setting { key: "mouse_aim_smoothing", label: "Mouse aim smoothing", domain: Domain::Flag },
        Setting { key: "ads_sensitivity_modifier", label: "ADS sensitivity modifier", domain: Domain::Decimal { minimum: 0.5, maximum: 1.5, step: 0.1 } },
        Setting { key: "double_press_delay", label: "Double-press delay", domain: Domain::Choice(DOUBLE_PRESS_DELAYS) },
    ],
};

pub(super) const AUDIO: SettingGroup = SettingGroup {
    name: "audio",
    heading: "Audio",
    description: "Voice, volume, and focus behavior.",
    settings: &[
        Setting { key: "voice_output_mode", label: "Voice output mode", domain: Domain::Choice(VOICE_OUTPUT_MODES) },
        Setting { key: "team_voice_channel", label: "Team voice channel", domain: Domain::Choice(TEAM_VOICE_MODES) },
        Setting { key: "reserved_mode", label: "Proximity voice output", domain: Domain::Choice(PROXIMITY_VOICE_OUTPUTS) },
        Setting { key: "migration_version", label: "Audio migration version", domain: Domain::Exact(8) },
        Setting { key: "chat_volume", label: "Voice chat volume", domain: Domain::Range { minimum: 0, maximum: 8 } },
        Setting { key: "mute_when_unfocused", label: "Mute when unfocused", domain: Domain::Flag },
        Setting { key: "sound_effects_volume", label: "Sound effects volume", domain: Domain::Range { minimum: 0, maximum: 10 } },
        Setting { key: "dialogue_volume", label: "Dialogue volume", domain: Domain::Range { minimum: 0, maximum: 10 } },
        Setting { key: "music_volume", label: "Music volume", domain: Domain::Range { minimum: 0, maximum: 10 } },
    ],
};

pub(super) const DISPLAY: SettingGroup = SettingGroup {
    name: "display",
    heading: "Display",
    description: "Brightness and display overlays. Renderer calibration is shown but kept at Sunrise's required values.",
    settings: &[
        Setting { key: "brightness", label: "Brightness", domain: Domain::Range { minimum: 0, maximum: 6 } },
        Setting { key: "show_fps", label: "Show FPS", domain: Domain::Flag },
        Setting { key: "hdr_mode", label: "HDR mode", domain: Domain::Choice(HDR_MODES) },
        Setting { key: "calibration_primary", label: "Renderer calibration", domain: Domain::ExactDecimal(10_000.0) },
        Setting { key: "calibration_alpha", label: "Renderer calibration alpha", domain: Domain::ExactDecimal(0.0) },
    ],
};

pub(super) const INTERFACE: SettingGroup = SettingGroup {
    name: "interface",
    heading: "Interface",
    description: "HUD, subtitle, reticle, and text presentation.",
    settings: &[
        Setting { key: "subtitles_mode", label: "Subtitles mode", domain: Domain::Choice(SUBTITLE_MODES) },
        Setting { key: "colorblind_mode", label: "Colorblind mode", domain: Domain::Choice(COLORBLIND_MODES) },
        Setting { key: "helmet_mode", label: "Helmet mode", domain: Domain::Choice(HELMET_MODES) },
        Setting { key: "hud_opacity", label: "HUD opacity", domain: Domain::Choice(HUD_OPACITY) },
        Setting { key: "display_hints", label: "Display hints", domain: Domain::Flag },
        Setting { key: "background_opacity", label: "Background opacity", domain: Domain::Choice(BACKGROUND_OPACITY) },
        Setting { key: "reticle_location", label: "Reticle location", domain: Domain::Choice(RETICLE_LOCATIONS) },
        Setting { key: "reticle_color", label: "Reticle color", domain: Domain::Range { minimum: 0, maximum: 6 } },
        Setting { key: "text_size", label: "Text size", domain: Domain::Range { minimum: 0, maximum: 4 } },
        Setting { key: "text_color", label: "Text color", domain: Domain::Range { minimum: 0, maximum: 3 } },
        Setting { key: "text_background_style", label: "Text background style", domain: Domain::Range { minimum: 0, maximum: 3 } },
        Setting { key: "text_background_opacity", label: "Text background opacity", domain: Domain::Range { minimum: 0, maximum: 4 } },
        Setting { key: "reserved_text_mode", label: "Reserved text mode", domain: Domain::Exact(0) },
        Setting { key: "subtitle_options_entry", label: "Subtitle options entry", domain: Domain::Exact(0) },
    ],
};

pub(super) const SOCIAL: SettingGroup = SettingGroup {
    name: "social",
    heading: "Social",
    description: "Chat, voice, names, and notifications.",
    settings: &[
        Setting { key: "prefer_good_connection", label: "Prefer good connection", domain: Domain::Flag },
        Setting { key: "text_chat_mode", label: "Text chat mode", domain: Domain::Choice(TEXT_CHAT_MODES) },
        Setting { key: "show_real_names", label: "Show real names", domain: Domain::Flag },
        Setting { key: "clan_invite_notifications", label: "Clan invite notifications", domain: Domain::Flag },
        Setting { key: "profanity_filter", label: "Profanity filter", domain: Domain::Flag },
        Setting { key: "voice_chat_enabled", label: "Voice chat enabled", domain: Domain::Flag },
        Setting { key: "whisper_chat_mode", label: "Whisper chat mode", domain: Domain::Choice(WHISPER_CHAT_MODES) },
        Setting { key: "team_chat_join_mode", label: "Team chat join mode", domain: Domain::Choice(MANUAL_AUTOMATIC) },
        Setting { key: "local_chat_join_mode", label: "Local chat join mode", domain: Domain::Choice(MANUAL_AUTOMATIC) },
        Setting { key: "clan_chat_join_mode", label: "Clan chat join mode", domain: Domain::Choice(MANUAL_AUTOMATIC) },
        Setting { key: "chat_auto_hide_mode", label: "Chat auto-hide mode", domain: Domain::Choice(AUTO_HIDE_MODES) },
    ],
};

pub(super) const ACTIONS: &[(&str, &str)] = &[
    ("fire", "Fire"),
    ("toggle_zoom", "Toggle zoom"),
    ("hold_zoom", "Hold zoom"),
    ("melee", "Melee"),
    ("grenade", "Grenade"),
    ("super", "Super"),
    ("reload", "Reload"),
    ("light_attack", "Light attack"),
    ("heavy_attack", "Heavy attack"),
    ("block", "Block"),
    ("switch_weapons", "Switch weapons"),
    ("next_weapon", "Next weapon"),
    ("previous_weapon", "Previous weapon"),
    ("primary_weapon", "Primary weapon"),
    ("special_weapon", "Special weapon"),
    ("heavy_weapon", "Heavy weapon"),
    ("move_forward", "Move forward"),
    ("move_backward", "Move backward"),
    ("move_left", "Move left"),
    ("move_right", "Move right"),
    ("jump", "Jump"),
    ("toggle_crouch", "Toggle crouch"),
    ("hold_crouch", "Hold crouch"),
    ("toggle_sprint", "Toggle sprint"),
    ("hold_sprint", "Hold sprint"),
    ("vehicle_boost", "Vehicle boost"),
    ("vehicle_brake", "Vehicle brake"),
    ("vehicle_zoom", "Vehicle zoom"),
    ("vehicle_fire_primary", "Vehicle primary fire"),
    ("vehicle_fire_secondary", "Vehicle secondary fire"),
    ("vehicle_exit", "Exit vehicle"),
    ("interact", "Interact"),
    ("highlight_player", "Highlight player"),
    ("emote_1", "Emote 1"),
    ("emote_2", "Emote 2"),
    ("emote_3", "Emote 3"),
    ("emote_4", "Emote 4"),
    ("air_move", "Air move"),
    ("class_ability", "Class ability"),
    ("death_cam_zoom_in", "Death camera zoom in"),
    ("death_cam_zoom_out", "Death camera zoom out"),
    ("push_to_talk", "Push to talk"),
    ("ui_gamepad_button_back", "Gamepad back"),
    ("ui_open_director", "Open Director"),
    ("ui_open_director_store_tab", "Director: Store"),
    ("ui_open_director_pursuits_tab", "Director: Pursuits"),
    ("ui_open_director_map_tab", "Director: Map"),
    (
        "ui_open_director_destinations_tab",
        "Director: Destinations",
    ),
    ("ui_open_director_roster_tab", "Director: Roster"),
    ("ui_open_director_seasons_tab", "Director: Seasons"),
    ("ui_open_start_menu_alternative", "Open character menu"),
    ("ui_open_start_menu_records_tab", "Character menu: Records"),
    (
        "ui_open_start_menu_collections_tab",
        "Character menu: Collections",
    ),
    ("ui_open_start_menu_clan_tab", "Character menu: Clan"),
    (
        "ui_open_start_menu_inventory_tab",
        "Character menu: Inventory",
    ),
    (
        "ui_open_start_menu_settings_tab",
        "Character menu: Settings",
    ),
    ("ui_open_exit_dialog_confirm", "Confirm exit dialog"),
    ("ui_abort_activity", "Abort activity"),
    ("ui_text_chat_toggle_state", "Toggle text chat"),
    ("screenshot", "Screenshot"),
];
