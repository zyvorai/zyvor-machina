// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createContext, useContext, useState, useEffect, useCallback, ReactNode } from 'react'
import { login as apiLogin, logout as apiLogout, getSession, exchangeTokenForSession } from '../api/auth'
import { safePostLoginPath } from '../api/authRedirect'
import { clearAllVmHardwareCache } from '../hooks/useVmHardware'
import { clearAllKubevirtHardwareCache } from '../hooks/useKubevirtHardware'

interface AuthContextType {
  isAuthenticated: boolean
  username: string
  /** UNIX username is `root` (session administration). */
  isRoot: boolean
  sessionId: string
  loading: boolean
  login: (username: string, password: string) => Promise<void>
  exchangeToken: (token: string) => Promise<void>
  logout: () => Promise<void>
}

const AuthContext = createContext<AuthContextType>({
  isAuthenticated: false,
  username: '',
  isRoot: false,
  sessionId: '',
  loading: true,
  login: async () => {},
  exchangeToken: async () => {},
  logout: async () => {},
})

/** Login page is outside the router; honor `?next=` then mount the authenticated shell. */
function clearLoginPathFromUrl() {
  if (window.location.pathname === '/login') {
    const params = new URLSearchParams(window.location.search)
    const dest = safePostLoginPath(params.get('next'), '/')
    window.history.replaceState(null, '', dest)
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(false)
  const [username, setUsername] = useState('')
  const [sessionId, setSessionId] = useState('')
  const [loading, setLoading] = useState(true)

  // Check existing session on mount; honor ?token= platform JWT deep links.
  useEffect(() => {
    const params = new URLSearchParams(window.location.search)
    const token = params.get('token')?.trim()
    const bootstrap = async () => {
      try {
        if (token) {
          await exchangeTokenForSession(token)
          params.delete('token')
          const qs = params.toString()
          window.history.replaceState(null, '', `${window.location.pathname}${qs ? `?${qs}` : ''}`)
        }
        const session = await getSession()
        if (session.authenticated) clearLoginPathFromUrl()
        setIsAuthenticated(session.authenticated)
        setUsername(session.username || '')
        setSessionId(typeof session.session_id === 'string' ? session.session_id : '')
      } catch {
        if (token) {
          window.location.replace('/login?error=token')
        }
        setIsAuthenticated(false)
        setSessionId('')
      } finally {
        setLoading(false)
      }
    }
    void bootstrap()
  }, [])

  const login = useCallback(async (user: string, pass: string) => {
    await apiLogin(user, pass)
    const session = await getSession()
    clearLoginPathFromUrl()
    setIsAuthenticated(session.authenticated)
    setUsername(session.username || user)
    setSessionId(typeof session.session_id === 'string' ? session.session_id : '')
  }, [])

  const exchangeToken = useCallback(async (token: string) => {
    await exchangeTokenForSession(token)
    const session = await getSession()
    clearLoginPathFromUrl()
    setIsAuthenticated(session.authenticated)
    setUsername(session.username || '')
    setSessionId(typeof session.session_id === 'string' ? session.session_id : '')
  }, [])

  const logout = useCallback(async () => {
    await apiLogout()
    localStorage.removeItem('machina_platform_jwt')
    localStorage.removeItem('machina_platform_basic')
    localStorage.removeItem('machina-saved-login')
    // These VM-hardware caches are module-level singletons that outlive this
    // component tree, so a second user signing in on the same tab could
    // otherwise see a VM detail response cached under the previous session.
    clearAllVmHardwareCache()
    clearAllKubevirtHardwareCache()
    setIsAuthenticated(false)
    setUsername('')
    setSessionId('')
  }, [])

  return (
    <AuthContext.Provider value={{ isAuthenticated, username, isRoot: username === 'root', sessionId, loading, login, exchangeToken, logout }}>
      {children}
    </AuthContext.Provider>
  )
}

export function useAuth() {
  return useContext(AuthContext)
}
