import { Bridge } from '@iii-dev/sdk'

/**
 * Test worker metrics reporting
 * 
 * This example demonstrates:
 * 1. Creating a Bridge with metrics reporting enabled
 * 2. Registering a test function
 * 3. Waiting for metrics to accumulate
 * 4. Querying worker information with metrics
 */
async function testWorkerMetrics() {
  console.log('Starting worker metrics test...')
  
  // Create bridge with metrics reporting enabled
  const bridge = new Bridge('ws://localhost:49134', {
    workerName: 'test-metrics-worker',
    enableMetricsReporting: true,
    metricsReportIntervalMs: 2000,  // Report every 2 seconds for faster testing
  })
  
  // Register a test function to keep worker active
  bridge.registerFunction(
    {
      function_path: 'test.metrics.echo',
      description: 'Test function for metrics demo',
    },
    async (input: any) => {
      return { echo: input, timestamp: Date.now() }
    }
  )
  
  console.log('Worker registered, waiting for metrics to accumulate...')
  console.log('Metrics will be reported every 2 seconds')
  
  // Wait for metrics to accumulate
  await new Promise(resolve => setTimeout(resolve, 10000))
  
  console.log('\nQuerying worker information...')
  
  try {
    // Query all workers
    const result = await bridge.invokeFunction('engine.workers.list', {})
    
    if (result && result.workers) {
      console.log(`\nFound ${result.workers.length} worker(s):\n`)
      
      for (const worker of result.workers) {
        console.log(`Worker ID: ${worker.id}`)
        console.log(`  Name: ${worker.name}`)
        console.log(`  Runtime: ${worker.runtime}`)
        console.log(`  Status: ${worker.status}`)
        console.log(`  Functions: ${worker.function_count}`)
        
        if (worker.memory_heap_used) {
          console.log('\n  Resource Metrics:')
          console.log(`    Memory Heap Used: ${(worker.memory_heap_used / 1_048_576).toFixed(2)} MB`)
          console.log(`    Memory Heap Total: ${(worker.memory_heap_total / 1_048_576).toFixed(2)} MB`)
          console.log(`    Memory RSS: ${(worker.memory_rss / 1_048_576).toFixed(2)} MB`)
          console.log(`    CPU Usage: ${worker.cpu_percent?.toFixed(2)}%`)
          
          if (worker.event_loop_lag_ms !== undefined) {
            console.log(`    Event Loop Lag: ${worker.event_loop_lag_ms.toFixed(2)} ms`)
          }
          
          console.log(`    Uptime: ${worker.uptime_seconds} seconds`)
          console.log(`    Last Update: ${new Date(worker.last_metrics_update_ms).toISOString()}`)
        } else {
          console.log('\n  No metrics available yet (worker may have just connected)')
        }
        
        console.log()
      }
      
      // Test with history
      console.log('Querying worker metrics with history via REST API...')
      console.log('(In a real application, you would use fetch or axios)')
      console.log('Example: GET http://localhost:3111/api/workers?include_history=true')
      
    } else {
      console.log('No workers found')
    }
  } catch (error) {
    console.error('Error querying workers:', error)
  }
  
  console.log('\nTest complete! Worker will continue reporting metrics.')
  console.log('Press Ctrl+C to exit')
}

// Run the test
testWorkerMetrics().catch(console.error)
