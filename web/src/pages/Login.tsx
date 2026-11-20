// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useState, type FormEvent } from 'react'
import { useAuth } from '../contexts/AuthContext'
import { beginOidcLogin, getAuthProviders, type AuthProviders } from '../api/auth'
import { formatUserError } from '../utils/apiError'
import {
  Lock,
  User,
  Loader2,
  Eye,
  EyeOff,
  ArrowRight,
  ChevronLeft,
  CheckCircle,
} from 'lucide-react'
import {
  PremiumLoginShell,
  LoginField,
  LoginSubmit,
  LoginRemember,
  LoginError,
} from '../components/PremiumLoginShell'

type LoginStep = 'identify' | 'password'

export default function LoginPage() {
  const saved = (() => {
    try {
      const raw = localStorage.getItem('machina-saved-login')
      return raw ? (JSON.parse(raw) as { username?: string }) : null
    } catch {
      return null
    }
  })()

  const [step, setStep] = useState<LoginStep>(saved?.username ? 'password' : 'identify')
  const [username, setUsername] = useState(saved?.username ?? '')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [showPassword, setShowPassword] = useState(false)
  const [rememberMe, setRememberMe] = useState(!!saved)
  const [providers, setProviders] = useState<AuthProviders>({
    pam: { enabled: true },
    ldap: { enabled: false },
    oidc: { enabled: false, button_label: 'Sign in with SSO' },
    saml: { enabled: false, button_label: 'Sign in with SAML', login_available: false },
  })
  const { login } = useAuth()
  const loginDest = useMemo(() => {
    if (typeof window === 'undefined') return { host: '', origin: '', port: '', protocol: '' }
    const { hostname, origin, port, protocol } = window.location
    return {
      host: hostname || 'localhost',
      origin: origin || '',
      port: port || (protocol === 'https:' ? '443' : protocol === 'http:' ? '80' : ''),
      protocol: protocol.replace(':', '') || 'https',
    }
  }, [])
  const oidcEnabled = providers.oidc.enabled
  const ldapEnabled = providers.ldap.enabled

  useEffect(() => {
    document.title = `Sign in · machina · ${loginDest.host || 'cluster'}`
    return () => {
      document.title = 'machina'
    }
  }, [loginDest.host])

  useEffect(() => {
    void getAuthProviders().then(setProviders).catch(() => {})
    const params = new URLSearchParams(window.location.search)
    const errorParam = params.get('error')
    if (errorParam === 'oidc') {
      setError('SSO login failed')
    } else if (errorParam === 'token') {
      setError('Sign-in link is invalid or expired')
    } else if (errorParam === 'saml') {
      setError('SAML authentication failed')
    } else if (errorParam) {
      setError('Authentication failed — please try again')
    }
  }, [])

  const handleContinue = (e: FormEvent) => {
    e.preventDefault()
    if (!username.trim()) return
    setError('')
    setStep('password')
  }

  const handleBack = () => {
    setStep('identify')
    setPassword('')
    setShowPassword(false)
  }

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    if (!username.trim() || !password) return
    setSubmitting(true)
    setError('')
    try {
      await login(username.trim(), password)
      if (rememberMe) {
        localStorage.setItem('machina-saved-login', JSON.stringify({ username: username.trim() }))
      } else {
        localStorage.removeItem('machina-saved-login')
      }
    } catch (e: unknown) {
      setError(formatUserError(e) || 'Login failed')
    } finally {
      setSubmitting(false)
    }
  }

  const panelSubtitle =
    step === 'password' ? (
      <>
        Enter the password for <span className="login-apple-host">{username.trim()}</span>
      </>
    ) : (
      'Sign in'
    )

  return (
    <PremiumLoginShell
      productName="machina"
      productWordmark="machina"
      hostBadge={loginDest.host || undefined}
      heroTitle={
        <>
          Sign in to
          <br />
          machina.
        </>
      }
      heroSubheadline={
        loginDest.host
          ? `Private cloud on ${loginDest.host}. Enter your credentials to continue.`
          : 'Private cloud control plane. Enter your credentials to continue.'
      }
      chapterNote={null}
      panelSubtitle={panelSubtitle}
      panelHint={
        step === 'identify' ? (
          ldapEnabled ? (
            <>
              Sign in with your <span className="font-mono">directory (LDAP/AD)</span> username, or a
              PAM system account on this host.
            </>
          ) : (
            <>
              Use a <span className="font-mono">system account on this host</span> (PAM).
            </>
          )
        ) : null
      }
      showSignInChapter
    >
      {step === 'identify' ? (
        <form
          key="identify"
          onSubmit={handleContinue}
          autoComplete="on"
          aria-label="Account"
          className="login-apple-step text-left"
        >
          {error ? <LoginError message={error} /> : null}

          {providers.saml?.enabled && !providers.saml.login_available ? (
            <p className="text-xs text-zinc-400 mb-4">
              SAML metadata is configured for IdP federation — use SSO or your local account below.
            </p>
          ) : null}

          <div className="login-apple-fields">
            <LoginField label="Username" id="username">
              <User className="login-field-icon" />
              <input
                id="username"
                name="username"
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                className="login-input"
                placeholder="System username"
                autoComplete="username"
                required
              />
            </LoginField>
          </div>

          <LoginSubmit loading={false} disabled={!username.trim()}>
            <span>Continue</span>
            <ArrowRight className="h-4 w-4" />
          </LoginSubmit>

          {oidcEnabled ? (
            <button
              type="button"
              onClick={() => beginOidcLogin()}
              className="mt-4 flex items-center justify-center w-full text-sm text-[#0071e3] hover:underline"
            >
              {providers.oidc.button_label}
            </button>
          ) : null}
        </form>
      ) : (
        <form
          key="password"
          onSubmit={handleSubmit}
          autoComplete="on"
          aria-label="Password"
          className="login-apple-step text-left"
        >
          <button type="button" onClick={handleBack} className="login-apple-identity" aria-label="Change account">
            <ChevronLeft aria-hidden className="h-4 w-4 shrink-0" />
            <span className="truncate">{username.trim()}</span>
          </button>
          <input type="text" name="username" value={username} autoComplete="username" readOnly hidden />

          {error ? <LoginError message={error} /> : null}

          <div className="login-apple-fields">
            <LoginField label="Password" id="password">
              <Lock className="login-field-icon" />
              <input
                id="password"
                name="password"
                type={showPassword ? 'text' : 'password'}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="login-input pr-11"
                placeholder="Password"
                autoComplete="current-password"
                autoFocus
                required
                disabled={submitting}
              />
              <button
                type="button"
                onClick={() => setShowPassword(!showPassword)}
                className="absolute right-3.5 top-1/2 -translate-y-1/2 text-zinc-400 hover:text-zinc-700 transition-colors"
                aria-label={showPassword ? 'Hide password' : 'Show password'}
              >
                {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
              </button>
            </LoginField>
          </div>

          <LoginRemember
            checked={rememberMe}
            onChange={(checked) => {
              setRememberMe(checked)
              if (!checked) localStorage.removeItem('machina-saved-login')
            }}
            label="Remember me on this device"
          />

          <LoginSubmit loading={submitting} disabled={!password}>
            {submitting ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                <span>Signing in…</span>
              </>
            ) : (
              <>
                <span>Sign In</span>
                <ArrowRight className="h-4 w-4" />
              </>
            )}
          </LoginSubmit>

          {oidcEnabled ? (
            <button
              type="button"
              onClick={() => beginOidcLogin()}
              className="mt-4 flex items-center justify-center w-full text-sm text-[#0071e3] hover:underline"
            >
              {providers.oidc.button_label}
            </button>
          ) : null}

          <div className="flex items-center justify-center gap-1.5 mt-4 text-xs text-zinc-400">
            <CheckCircle className="h-3.5 w-3.5" />
            <span>Secured with system PAM authentication</span>
          </div>
        </form>
      )}
    </PremiumLoginShell>
  )
}
