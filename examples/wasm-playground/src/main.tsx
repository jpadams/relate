import React from 'react'
import ReactDOM from 'react-dom/client'
import { NeedleThemeProvider } from '@neo4j-ndl/react/theme'
import '@neo4j-ndl/base/lib/neo4j-ds-styles.css'
import { App } from './App'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <NeedleThemeProvider theme="dark">
      <App />
    </NeedleThemeProvider>
  </React.StrictMode>,
)
