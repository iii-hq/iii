import { Badge, Button } from '@motiadev/ui'
import { Sparkles } from 'lucide-react'
import type React from 'react'

export const ExamplePage: React.FC = () => {
  return (
    <div className="h-full w-full p-6 overflow-auto">
      <div className="max-w-4xl mx-auto space-y-6">
        <div className="flex items-center gap-3">
          <Sparkles className="w-8 h-8 text-primary" />
          <h1 className="text-3xl font-bold">Example Plugin</h1>
          <Badge variant="info">v1.0.0</Badge>
        </div>

        <p className="text-muted-foreground text-lg">
          Welcome to the example plugin! This demonstrates the basic structure and functionality of a Motia plugin.
        </p>

        <div className="p-6 space-y-4">
          <h2 className="text-xl font-semibold">What is this?</h2>
          <p className="text-muted-foreground">
            This is a minimal example plugin that shows how to create custom workbench tabs in Motia. Plugins can extend
            the Motia workbench with custom functionality, visualizations, and tools.
          </p>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mt-6">
            <div className="p-4 border rounded-lg">
              <h3 className="font-semibold mb-2">Easy to Create</h3>
              <p className="text-sm text-muted-foreground">Build plugins with React, TypeScript, and Tailwind CSS</p>
            </div>
            <div className="p-4 border rounded-lg">
              <h3 className="font-semibold mb-2">Integrated</h3>
              <p className="text-sm text-muted-foreground">Seamlessly integrate with Motia's workbench UI</p>
            </div>
            <div className="p-4 border rounded-lg">
              <h3 className="font-semibold mb-2">Powerful</h3>
              <p className="text-sm text-muted-foreground">Access Motia's plugin context and APIs</p>
            </div>
          </div>

          <div className="flex gap-2 mt-6">
            <Button variant="default">
              <Sparkles className="w-4 h-4" />
              Get Started
            </Button>
            <Button variant="outline">View Documentation</Button>
          </div>
        </div>

        <div className="p-6 space-y-4">
          <h2 className="text-xl font-semibold">Plugin Features</h2>
          <ul className="list-disc list-inside space-y-2 text-muted-foreground">
            <li>Custom workbench tabs with position control (top/bottom)</li>
            <li>Access to Motia's UI components library</li>
            <li>Integration with state management and APIs</li>
            <li>Real-time updates through streams</li>
            <li>TypeScript support with full type safety</li>
          </ul>
        </div>
      </div>
    </div>
  )
}
