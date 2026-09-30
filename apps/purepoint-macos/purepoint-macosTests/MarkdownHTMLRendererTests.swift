import Testing

@testable import purepoint_macos

@Suite struct MarkdownHTMLRendererTests {
    @Test func rendersHeadingAndParagraph() {
        let html = MarkdownHTMLRenderer.render("# Title\n\nhello **bold** and `code`")
        #expect(html.contains("<h1>Title</h1>"))
        #expect(html.contains("<strong>bold</strong>"))
        #expect(html.contains("<code>code</code>"))
    }

    @Test func escapesRawHTML() {
        let html = MarkdownHTMLRenderer.render("<script>alert(1)</script>")
        #expect(!html.contains("<script>"))
        #expect(html.contains("&lt;script&gt;"))
    }

    @Test func neutralizesJavascriptLinks() {
        let html = MarkdownHTMLRenderer.render("[x](javascript:alert(1))")
        #expect(!html.contains("javascript:"))
    }

    @Test func fencedCodeIsVerbatim() {
        let html = MarkdownHTMLRenderer.render("```swift\nlet a = **1**\n```")
        #expect(html.contains("<pre><code class=\"language-swift\">let a = **1**</code></pre>"))
    }

    @Test func rendersListsAndTables() {
        let html = MarkdownHTMLRenderer.render("- a\n- b\n\n| h |\n|---|\n| c |")
        #expect(html.contains("<ul><li>a</li><li>b</li></ul>"))
        #expect(html.contains("<th>h</th>"))
        #expect(html.contains("<td>c</td>"))
    }

    @Test func codeSpanContentsAreNotEmphasized() {
        let html = MarkdownHTMLRenderer.render("`*x*`")
        #expect(html.contains("<code>*x*</code>"))
    }
}
