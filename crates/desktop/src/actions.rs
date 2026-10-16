//! Every command the menu bar, the key bindings and the context menus share. Menus are built
//! from these actions, so a menu item and its shortcut never disagree; `root.rs` handles them.

use messages_core::TapbackKind;
use nd::prelude::*;

pub const CONTEXT: &str = "Messages";

actions!(
    messages,
    [
        NewChat,
        OpenSwitcher,
        FocusSearch,
        ToggleInfo,
        OpenSettings,
        MarkUnread,
        NextConversation,
        PrevConversation,
        Dismiss,
        ReportIssue,
    ]
);

/// Cmd/Ctrl+1..6: the n-th tapback on the newest incoming message.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = messages, no_json)]
pub struct QuickTapback(pub usize);

/// One conversation, by its primary guid.
macro_rules! chat_actions {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Clone, PartialEq, Debug, Action)]
        #[action(namespace = messages, no_json)]
        pub struct $name(pub String);
    )*};
}

chat_actions!(
    OpenChat,
    TogglePin,
    ToggleMute,
    ToggleReceipts,
    MarkChatRead,
    MarkChatUnread,
    ShowDetails,
    ExportChat,
    DeleteChat
);

/// One message in one chat.
#[derive(Clone, PartialEq, Debug)]
pub struct MessageRef {
    pub chat: String,
    pub message: String,
}

macro_rules! message_actions {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Clone, PartialEq, Debug, Action)]
        #[action(namespace = messages, no_json)]
        pub struct $name(pub MessageRef);
    )*};
}

message_actions!(Reply, CopyText, Translate, Edit, Unsend, Retry, NotifyAnyway, OpenThread, JumpTo, OpenTapbacks);

actions!(
    messages,
    [CloseTapbacks, LightboxPrev, LightboxNext, LightboxCopy, LightboxPlay, LightboxMute, RemoveLastRecipient]
);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = messages, no_json)]
pub struct React {
    pub at: MessageRef,
    pub kind: TapbackKind,
}

/// An attachment of a message: open it, copy it, copy its name.
#[derive(Clone, PartialEq, Debug)]
pub struct AttachmentRef {
    pub at: MessageRef,
    pub attachment: String,
}

macro_rules! attachment_actions {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Clone, PartialEq, Debug, Action)]
        #[action(namespace = messages, no_json)]
        pub struct $name(pub AttachmentRef);
    )*};
}

attachment_actions!(OpenAttachment, CopyAttachment, CopyAttachmentName, ShowAttachment, Transcribe, Download);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = messages, no_json)]
pub struct OpenLink(pub String);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = messages, no_json)]
pub struct CopyAddress(pub String);

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = messages, no_json)]
pub struct RemoveParticipant {
    pub chat: String,
    pub address: String,
}

/// Schedule the composer's text in `minutes` from now.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = messages, no_json)]
pub struct ScheduleIn(pub ScheduleWhen);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScheduleWhen {
    InAnHour,
    Tonight,
    TomorrowMorning,
    Pick,
}

pub fn init(cx: &mut App) {
    cx.bind_keys(nd::actions::standard_bindings());
    let mut bindings = vec![
        KeyBinding::new("secondary-n", NewChat, Some(CONTEXT)),
        KeyBinding::new("secondary-k", OpenSwitcher, Some(CONTEXT)),
        KeyBinding::new("secondary-f", FocusSearch, Some(CONTEXT)),
        KeyBinding::new("secondary-i", ToggleInfo, Some(CONTEXT)),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-shift-u", MarkUnread, Some(CONTEXT)),
        KeyBinding::new("secondary-]", NextConversation, Some(CONTEXT)),
        KeyBinding::new("secondary-shift-down", NextConversation, Some(CONTEXT)),
        KeyBinding::new("secondary-[", PrevConversation, Some(CONTEXT)),
        KeyBinding::new("secondary-shift-up", PrevConversation, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
    ];
    for n in 1..=6 {
        bindings.push(KeyBinding::new(&format!("secondary-{n}"), QuickTapback(n), Some(CONTEXT)));
    }
    bindings.extend([
        KeyBinding::new("left", LightboxPrev, Some("Lightbox")),
        KeyBinding::new("k", LightboxPrev, Some("Lightbox")),
        KeyBinding::new("right", LightboxNext, Some("Lightbox")),
        KeyBinding::new("j", LightboxNext, Some("Lightbox")),
        KeyBinding::new("secondary-c", LightboxCopy, Some("Lightbox")),
        KeyBinding::new("space", LightboxPlay, Some("Lightbox")),
        KeyBinding::new("m", LightboxMute, Some("Lightbox")),
        KeyBinding::new("backspace", RemoveLastRecipient, Some("NewChatTo")),
    ]);
    cx.bind_keys(bindings);
    set_menus(
        cx,
        [
            Menu::app([MenuItem::action("Settings\u{2026}", OpenSettings)]),
            Menu::new("File").items([MenuItem::action("New Message", NewChat)]),
            Menu::edit([]),
            Menu::new("Conversation").items([
                MenuItem::action("Go to Conversation\u{2026}", OpenSwitcher),
                MenuItem::action("Search", FocusSearch),
                MenuItem::separator(),
                MenuItem::action("Next Conversation", NextConversation),
                MenuItem::action("Previous Conversation", PrevConversation),
                MenuItem::separator(),
                MenuItem::action("Mark as Unread", MarkUnread),
                MenuItem::action("Show Details", ToggleInfo),
            ]),
            Menu::window([]),
            Menu::help([MenuItem::action("Report an Issue\u{2026}", ReportIssue)]),
        ],
    );
}
