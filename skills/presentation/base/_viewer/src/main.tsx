import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@lib/index.css'
import { ViewerApp } from './ViewerApp'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ViewerApp />
  </StrictMode>,
)
