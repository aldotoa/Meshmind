//! Print the tool reference: `cargo run -p meshmind-mcp --example tool_docs > docs/TOOLS.md`
fn main() {
    print!("{}", meshmind_mcp::tool_reference_markdown());
}
