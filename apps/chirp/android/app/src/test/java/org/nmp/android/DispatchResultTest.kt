package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.nmp.android.model.SnapshotProjections
import org.nmp.android.model.testJson
import kotlinx.serialization.decodeFromString

class DispatchResultTest {
    @Test
    fun acceptedEnvelopeParsesCorrelationId() {
        assertEquals(
            DispatchResult.Accepted("abc123"),
            DispatchResult.parse("""{"correlation_id":"abc123"}"""),
        )
    }

    @Test
    fun errorEnvelopeParsesFailure() {
        assertEquals(
            DispatchResult.Failure("bad action"),
            DispatchResult.parse("""{"error":"bad action"}"""),
        )
    }

    @Test
    fun malformedEnvelopeFailsClosed() {
        assertTrue(DispatchResult.parse("{bad") is DispatchResult.Failure)
        assertTrue(DispatchResult.parse("{}") is DispatchResult.Failure)
    }

    @Test
    fun actionStageProjectionDecodes() {
        val projections = testJson().decodeFromString<SnapshotProjections>(
            """
            {
              "action_results": [
                {"correlation_id":"c1","status":"published","error":null}
              ],
              "action_stages": {
                "c1": [
                  {"stage":"requested","at_ms":10},
                  {"stage":"accepted","at_ms":11}
                ]
              },
              "action_lifecycle": {
                "in_flight": [
                  {"correlation_id":"c2","stage":"publishing"}
                ],
                "recent_terminal": [
                  {"correlation_id":"c1","stage":"accepted"}
                ]
              }
            }
            """.trimIndent(),
        )

        assertEquals("c1", projections.actionResults.single().correlationId)
        assertEquals("accepted", projections.actionStages.getValue("c1").last().stage)
        assertEquals("c2", projections.actionLifecycle?.inFlight?.single()?.correlationId)
        assertEquals("accepted", projections.actionLifecycle?.recentTerminal?.single()?.stage)
    }
}
