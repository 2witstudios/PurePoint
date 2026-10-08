import Testing
@testable import PurePoint

@MainActor
struct CommandPaletteOpenCoordinatorTests {
    @Test func sameTurnRequestsOnlyOpenOnce() async throws {
        let coordinator = CommandPaletteOpenCoordinator()
        var opens = 0
        let first = try #require(coordinator.request { opens += 1 })
        let duplicate = coordinator.request { opens += 1 }
        #expect(duplicate == nil)
        await first.value
        #expect(opens == 1)
    }

    @Test func requestsDuringLoadingCoalesceAndLaterRequestsCanOpen() async throws {
        let coordinator = CommandPaletteOpenCoordinator()
        let started = AsyncStream<Void>.makeStream()
        let resume = AsyncStream<Void>.makeStream()
        var loads = 0
        var opens = 0

        let first = try #require(
            coordinator.request {
                loads += 1
                started.continuation.yield(())
                var iterator = resume.stream.makeAsyncIterator()
                _ = await iterator.next()
                opens += 1
            })
        var startIterator = started.stream.makeAsyncIterator()
        _ = await startIterator.next()

        // Different entry points use this same coordinator; retries must not load or toggle.
        for _ in 0..<3 {
            let retry = coordinator.request {
                loads += 1
                opens += 1
            }
            #expect(retry == nil)
        }
        #expect(loads == 1)
        #expect(opens == 0)

        resume.continuation.yield(())
        await first.value
        #expect(opens == 1)

        let next = try #require(coordinator.request { opens += 1 })
        await next.value
        #expect(opens == 2)
        started.continuation.finish()
        resume.continuation.finish()
    }
}
