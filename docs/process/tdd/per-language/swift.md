# TDD — Swift

Swift-specific TDD conventions for PurePoint.app (macOS desktop application).

## Test Framework

- XCTest (built-in)
- `Cmd+U` in Xcode or `swift test` from command line
- Test targets in Xcode project

## Test Organization

- Test files in the test target, mirroring source structure
- Name test classes as `{ClassName}Tests`
- Name test methods as `test_{given}_{should}` or `testGiven{Situation}Should{Behavior}`

## Assert Format (Swift Adaptation)

```swift
func testGivenNewProjectShouldShowInSidebar() {
    // given
    let project = Project(path: "/tmp/test-repo")
    let sidebar = SidebarView()
    
    // when
    sidebar.addProject(project)
    
    // then
    XCTAssertEqual(sidebar.projects.count, 1)
    XCTAssertEqual(sidebar.projects.first?.path, "/tmp/test-repo")
}
```

## SwiftTerm Testing

- Terminal views are hard to unit test — focus on the data layer
- Test `DaemonClient` JSON serialization/deserialization separately from UI
- Mock the Unix socket connection for client tests
- Use XCTest expectations for async IPC operations

## UI Testing

- Use XCUITest for critical user journeys only (expensive to maintain)
- Prefer testing view models and controllers over views directly
- Mock WorkspaceService protocol and DaemonClient in view model tests

## Daemon Client Testing

- Test NDJSON request/response serialization (encode request → JSON line, decode response → typed struct)
- Test attach-mode streaming (partial reads, multiple messages per chunk, connection drop mid-stream)
- Test error paths (connection lost, timeout, JSON parse error). No external dependencies — the protocol is plain JSON over Unix socket
