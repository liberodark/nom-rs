use crate::ansi;

#[derive(Debug, Clone)]
pub struct TreeNode<T> {
    pub label: T,
    pub children: Vec<TreeNode<T>>,
}

/// Tag a node based on its position in the tree (root, twig, leaf).
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TreeLocation {
    Root,
    Twig,
    Leaf,
}

/// Map a `TreeNode<A> -> TreeNode<B>` using different mapping functions per location.
pub fn map_roots_twigs_leaves<A, B>(
    tree: TreeNode<A>,
    map_root: &impl Fn(A) -> B,
    map_twig: &impl Fn(A) -> B,
    map_leaf: &impl Fn(A) -> B,
) -> TreeNode<B> {
    go(tree, true, map_root, map_twig, map_leaf)
}

fn go<A, B>(
    node: TreeNode<A>,
    top: bool,
    map_root: &impl Fn(A) -> B,
    map_twig: &impl Fn(A) -> B,
    map_leaf: &impl Fn(A) -> B,
) -> TreeNode<B> {
    if node.children.is_empty() {
        TreeNode {
            label: map_leaf(node.label),
            children: Vec::new(),
        }
    } else {
        let label = if top {
            map_root(node.label)
        } else {
            map_twig(node.label)
        };
        let children = node
            .children
            .into_iter()
            .map(|c| go(c, false, map_root, map_twig, map_leaf))
            .collect();
        TreeNode { label, children }
    }
}

/// Render a forest into a vector of lines.
pub fn show_forest(forest: &[TreeNode<String>]) -> Vec<String> {
    let mut out = Vec::new();
    for t in forest {
        render(t, &mut out, "", "");
    }
    out
}

fn render(node: &TreeNode<String>, out: &mut Vec<String>, prefix: &str, cont_prefix: &str) {
    out.push(format!("{prefix}{}", node.label));
    let last_i = node.children.len().saturating_sub(1);
    for (i, child) in node.children.iter().enumerate() {
        let is_last = i == last_i;
        let branch = if is_last {
            ansi::wrap(ansi::BLUE, "┗━ ")
        } else {
            ansi::wrap(ansi::BLUE, "┣━ ")
        };
        let cont = if is_last {
            "   ".to_string()
        } else {
            ansi::wrap(ansi::BLUE, "┃  ")
        };
        let new_prefix = format!("{cont_prefix}{branch}");
        let new_cont = format!("{cont_prefix}{cont}");
        render(child, out, &new_prefix, &new_cont);
    }
}
