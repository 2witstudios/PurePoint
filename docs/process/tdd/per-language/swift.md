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
    let sidebar = SidebarViewController()
    
    // when
    sidebar.addProject(project)
    
    // then
    XCTAssertEqual(sidebar.projects.count, 1)
    XCTAssertEqual(sidebar.projects.first?.path, "/tmp/test-repo")
}
```

## SwiftTerm Testing

- Terminal views are hard to unit test — focus on the data layer
- Test gRPC client logic separately from UI
- Mock the gRPC channel for client tests
- Use XCTest expectations for async gRPC operations

## UI Testing

- Use XCUITest for critical user journeys only (expensive to maintain)
- Prefer testing view models and controllers over views directly
- Mock DashboardSession and PPGService in view controller tests

## gRPC Client Testing

- Use grpc-swift's test utilities for mock servers
- Test request serialization and response handling
- Test error handling paths (connection lost, timeout, invalid response)
