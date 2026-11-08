//! Native menus and keyboard bindings share the workspace command dispatcher.
use crate::{
    app::{Tool, Workspace},
    input,
};
use gpui::{prelude::*, *};
use pathweaver_core::appearance::Kind;

actions!(
    pathweaver,
    [
        NewDocument,
        Open,
        Save,
        SaveAs,
        ExportSvg,
        Close,
        Quit,
        Hide,
        HideOthers,
        Minimize,
        ZoomWindow,
        Duplicate,
        DeleteSelection,
        FitArt,
        ActualSize,
        ToggleNodes,
        ToggleTheme,
        SelectTool,
        DirectTool,
        ConvertTool,
        PenTool,
        WidthTool,
        Separate,
        Join,
        MergeCenter,
        Straighten,
        Union,
        AddNode,
        FitNodes,
        DuplicateNodes,
        DeleteNodes,
        ToggleNode
    ]
);

#[derive(Clone, Copy)]
pub enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Export,
    Close,
    Undo,
    Redo,
    SelectAll,
    Duplicate,
    Delete,
    FitArt,
    ActualSize,
    ToggleNodes,
    ToggleTheme,
    SelectTool,
    DirectTool,
    ConvertTool,
    PenTool,
    WidthTool,
    Separate,
    Join,
    MergeCenter,
    Straighten,
    Union,
    AddNode,
    FitNodes,
    DuplicateNodes,
    DeleteNodes,
    ToggleNode,
}

pub fn initialize(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-n", NewDocument, Some("PathWeaver")),
        KeyBinding::new("cmd-o", Open, Some("PathWeaver")),
        KeyBinding::new("cmd-s", Save, Some("PathWeaver")),
        KeyBinding::new("cmd-shift-s", SaveAs, Some("PathWeaver")),
        KeyBinding::new("cmd-e", ExportSvg, Some("PathWeaver")),
        KeyBinding::new("cmd-w", Close, Some("PathWeaver")),
        KeyBinding::new("cmd-q", Quit, Some("PathWeaver")),
        KeyBinding::new("cmd-z", input::Undo, Some("PathWeaver")),
        KeyBinding::new("cmd-shift-z", input::Redo, Some("PathWeaver")),
        KeyBinding::new("cmd-a", input::SelectAll, Some("PathWeaver")),
        KeyBinding::new("cmd-d", Duplicate, Some("PathWeaver")),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, Some("PathWeaver")),
    ]);
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    let menu = |name: &str, items| Menu {
        name: name.to_owned().into(),
        items,
    };
    cx.set_menus(vec![
        menu(
            "PathWeaver",
            vec![
                MenuItem::os_submenu("サービス", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("PathWeaverを隠す", Hide),
                MenuItem::action("ほかを隠す", HideOthers),
                MenuItem::separator(),
                MenuItem::action("PathWeaverを終了", Quit),
            ],
        ),
        menu(
            "ファイル",
            vec![
                MenuItem::action("新規", NewDocument),
                MenuItem::action("開く…", Open),
                MenuItem::separator(),
                MenuItem::action("保存", Save),
                MenuItem::action("別名で保存…", SaveAs),
                MenuItem::action("SVGを書き出す…", ExportSvg),
                MenuItem::separator(),
                MenuItem::action("ウィンドウを閉じる", Close),
            ],
        ),
        menu(
            "編集",
            vec![
                MenuItem::action("元に戻す", input::Undo),
                MenuItem::action("やり直す", input::Redo),
                MenuItem::separator(),
                MenuItem::action("切り取り", input::Cut),
                MenuItem::action("コピー", input::Copy),
                MenuItem::action("ペースト", input::Paste),
                MenuItem::action("すべてを選択", input::SelectAll),
                MenuItem::separator(),
                MenuItem::action("複製", Duplicate),
                MenuItem::action("選択を削除", DeleteSelection),
            ],
        ),
        menu(
            "表示",
            vec![
                MenuItem::action("作品全体を表示", FitArt),
                MenuItem::action("100%", ActualSize),
                MenuItem::separator(),
                MenuItem::action("ノードエディターを表示／隠す", ToggleNodes),
                MenuItem::action("ライト／ダークを切り替え", ToggleTheme),
            ],
        ),
        menu(
            "パス",
            vec![
                MenuItem::action("選択ツール", SelectTool),
                MenuItem::action("制御点ツール", DirectTool),
                MenuItem::action("アンカーポイントツール", ConvertTool),
                MenuItem::action("ペンツール", PenTool),
                MenuItem::action("幅ツール", WidthTool),
                MenuItem::separator(),
                MenuItem::action("選択した点で分離", Separate),
                MenuItem::action("選択した端点を連結", Join),
                MenuItem::action("選択した点を中心にマージ", MergeCenter),
                MenuItem::action("選択した区間を直線に", Straighten),
                MenuItem::action("選択したパスを合体", Union),
            ],
        ),
        menu(
            "ノード",
            vec![
                MenuItem::action("ノードを追加…", AddNode),
                MenuItem::action("全ノードを表示", FitNodes),
                MenuItem::separator(),
                MenuItem::action("選択したノードを複製", DuplicateNodes),
                MenuItem::action("選択したノードを削除", DeleteNodes),
                MenuItem::action("選択したノードを有効／無効", ToggleNode),
            ],
        ),
        menu(
            "ウィンドウ",
            vec![
                MenuItem::action("しまう", Minimize),
                MenuItem::action("拡大／縮小", ZoomWindow),
            ],
        ),
    ]);
}

pub fn handlers(mut root: Div, s: &Workspace, cx: &mut Context<Workspace>) -> Div {
    macro_rules! on {
        ($action:ty, $command:ident) => {
            root = root.on_action(
                cx.listener(|s, _: &$action, w, cx| s.menu_command(Command::$command, w, cx)),
            );
        };
    }
    on!(NewDocument, New);
    on!(Open, Open);
    on!(Save, Save);
    on!(SaveAs, SaveAs);
    on!(ExportSvg, Export);
    on!(Close, Close);
    on!(Quit, Close);
    // Focused text inputs handle these first; workspace history is the fallback.
    if s.session.can_undo() || s.tweeq_panels.focused() {
        on!(input::Undo, Undo);
    }
    if s.session.can_redo() || s.tweeq_panels.focused() {
        on!(input::Redo, Redo);
    }
    on!(input::SelectAll, SelectAll);
    on!(FitArt, FitArt);
    on!(ActualSize, ActualSize);
    on!(ToggleNodes, ToggleNodes);
    on!(ToggleTheme, ToggleTheme);
    on!(SelectTool, SelectTool);
    on!(DirectTool, DirectTool);
    on!(ConvertTool, ConvertTool);
    on!(PenTool, PenTool);
    on!(WidthTool, WidthTool);
    if s.tweeq_panels.focused() {
        root = root
            .on_action(cx.listener(|s, _: &input::Copy, _, _| s.tweeq_panels.command("copy")))
            .on_action(cx.listener(|s, _: &input::Cut, _, _| s.tweeq_panels.command("cut")))
            .on_action(cx.listener(|s, _: &input::Paste, _, _| s.tweeq_panels.command("paste")));
    }
    let editing = s.editing.is_some() || s.add_open || s.tweeq_panels.focused();
    let nodes_context = s.active_nodes && s.nodes_open;
    let editable_nodes = s.session.graph().is_some_and(|g| {
        s.session.node_selection.iter().any(|id| {
            g.node(*id)
                .is_some_and(|n| !matches!(n.kind, Kind::Input | Kind::Output))
        })
    });
    if !editing {
        if if nodes_context {
            editable_nodes
        } else {
            !s.session.selected.is_empty()
        } {
            on!(Duplicate, Duplicate);
        }
        let can_delete = if nodes_context {
            editable_nodes
        } else if matches!(s.tool, Tool::Direct | Tool::Convert) {
            !s.session.anchors.is_empty()
        } else {
            !s.session.selected.is_empty()
        };
        if can_delete {
            on!(DeleteSelection, Delete);
        }
        if s.session.selected.len() >= 2 {
            on!(Union, Union);
        }
        let anchors = &s.session.anchors;
        let anchor_node = |at: &pathweaver_core::editing::Anchor| {
            s.session
                .doc
                .strokes
                .iter()
                .find(|path| path.id == at.stroke && path.visible && path.contours.is_none())
                .and_then(|path| {
                    path.nodes
                        .iter()
                        .position(|node| node.id == at.node)
                        .map(|index| (path, index))
                })
        };
        if anchors.len() == 1
            && anchor_node(&anchors[0]).is_some_and(|(path, index)| {
                path.nodes.len() >= 2
                    && (path.closed || (index > 0 && index + 1 < path.nodes.len()))
            })
        {
            on!(Separate, Separate);
        }
        if anchors.len() == 2
            && anchors.iter().all(|at| {
                anchor_node(at).is_some_and(|(path, index)| {
                    !path.closed && (index == 0 || index + 1 == path.nodes.len())
                })
            })
        {
            on!(Join, Join);
        }
        if anchors.len() >= 2 {
            on!(MergeCenter, MergeCenter);
        }
        if s.session
            .segment
            .or_else(|| anchors.last().copied())
            .and_then(|at| anchor_node(&at))
            .is_some_and(|(path, index)| index < path.segment_count())
        {
            on!(Straighten, Straighten);
        }
    }
    if s.session.graph().is_some() {
        on!(AddNode, AddNode);
        on!(FitNodes, FitNodes);
        if editable_nodes && !editing {
            on!(DuplicateNodes, DuplicateNodes);
            on!(DeleteNodes, DeleteNodes);
            on!(ToggleNode, ToggleNode);
        }
    }
    root.on_action(cx.listener(|_, _: &Minimize, w, _| w.minimize_window()))
        .on_action(cx.listener(|_, _: &ZoomWindow, w, _| w.zoom_window()))
}
