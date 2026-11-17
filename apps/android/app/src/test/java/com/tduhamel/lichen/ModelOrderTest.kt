package com.tduhamel.lichen

import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Kotlin runs a class's property initialisers and init blocks in the order they are written.
 * AppModel's init block sets Compose state (refreshAll and the rest): a state property declared
 * after it is still null then, and the app crashes at launch (build 195 did, on a phone, with
 * `blocked`). Debug runs and the JVM tests never build an AppModel, so only this keeps the order.
 */
class ModelOrderTest {
    @Test
    fun theModelsInitBlockComesAfterEveryProperty() {
        val source = File("src/main/java/com/tduhamel/lichen/model/AppModel.kt").readLines()
        val init = source.indexOfFirst { it == "    init {" }
        assertTrue("AppModel has its init block", init >= 0)
        val property = Regex("""^    (private |internal |protected |override |lateinit |@Volatile )*(val|var) \w+""")
        val after = source.drop(init).withIndex().filter { property.containsMatchIn(it.value) }
        assertTrue(
            "properties declared after AppModel's init block: " +
                after.joinToString { "line ${init + it.index + 1}: ${it.value.trim()}" },
            after.isEmpty(),
        )
    }
}
