// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react'
import ReactDOM from 'react-dom/client'
import './i18n'
import App from './App'
import AppErrorBoundary from './components/AppErrorBoundary'
import './styles/fonts.css'
import './styles/main.css'
import './styles/netra-look.css'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AppErrorBoundary surface="app">
      <App />
    </AppErrorBoundary>
  </React.StrictMode>,
)
