// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from './locales/en.json'
import es from './locales/es.json'

import { applyDocumentLocale, detectLang, setFormatLocale } from '../utils/localeFormat'

const lang = detectLang()

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    es: { translation: es },
  },
  lng: lang,
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
})

applyDocumentLocale(lang)
setFormatLocale(lang)
i18n.on('languageChanged', (l: string) => {
  applyDocumentLocale(l)
  setFormatLocale(l)
})

export default i18n
