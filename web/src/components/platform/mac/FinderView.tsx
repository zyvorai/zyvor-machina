// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { useCallback, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { ChevronLeft, ChevronRight, Columns3, ExternalLink, LayoutGrid, List, Search } from 'lucide-react'
import FinderPathBar, { type FinderPathSegment } from './FinderPathBar'
import Tooltip from '../../Tooltip'
import { usePlatformMacDesktop } from './PlatformMacDesktopContext'
import { openCenterPopout, useCenterPopout } from '../../../utils/platformCenterPopout'
import {
  FINDER_INSPECTOR_MAX,
  FINDER_INSPECTOR_MIN,
  loadFinderInspectorWidth,
  saveFinderInspectorWidth,
} from '../../../utils/finderInspectorWidth'

export type FinderViewMode = 'icons' | 'list' | 'columns'

export default function FinderView({
  title,
  search,
  onSearchChange,
  searchPlaceholder = 'Search…',
  viewMode,
  onViewModeChange,
  toolbarActions,
  listContent,
  columnsContent,
  inspector,
  showInspector = true,
  onBack,
  onForward,
  pathSegments,
  emptyState,
  isEmpty,
  allowedViewModes,
}: {
  title?: string
  search: string
  onSearchChange: (v: string) => void
  searchPlaceholder?: string
  viewMode: FinderViewMode
  onViewModeChange: (mode: FinderViewMode) => void
  toolbarActions?: ReactNode
  listContent: ReactNode
  columnsContent?: ReactNode
  inspector?: ReactNode
  showInspector?: boolean
  onBack?: () => void
  onForward?: () => void
  pathSegments?: FinderPathSegment[]
  emptyState?: ReactNode
  isEmpty?: boolean
  allowedViewModes?: FinderViewMode[]
}) {
  const navigate = useNavigate()
  const { inspectorVisible } = usePlatformMacDesktop()
  const { isPopout } = useCenterPopout()
  const location = useLocation()
  const showInspectorPane = showInspector && inspectorVisible && inspector
  const isColumns = viewMode === 'columns' && columnsContent
  const [inspectorWidth, setInspectorWidth] = useState(loadFinderInspectorWidth)
  const widthRef = useRef(inspectorWidth)
  widthRef.current = inspectorWidth
  const dragRef = useRef<{ startX: number; startW: number } | null>(null)

  const handleBack = onBack ?? (() => navigate(-1))
  const handleForward = onForward ?? (() => navigate(1))

  const onResizeMove = useCallback((e: MouseEvent) => {
    if (!dragRef.current) return
    const delta = dragRef.current.startX - e.clientX
    const next = Math.min(FINDER_INSPECTOR_MAX, Math.max(FINDER_INSPECTOR_MIN, dragRef.current.startW + delta))
    setInspectorWidth(next)
  }, [])

  const onResizeEnd = useCallback(() => {
    if (dragRef.current) saveFinderInspectorWidth(widthRef.current)
    dragRef.current = null
    window.removeEventListener('mousemove', onResizeMove)
    window.removeEventListener('mouseup', onResizeEnd)
  }, [onResizeMove])

  const startResize = (e: React.MouseEvent) => {
    e.preventDefault()
    dragRef.current = { startX: e.clientX, startW: inspectorWidth }
    window.addEventListener('mousemove', onResizeMove)
    window.addEventListener('mouseup', onResizeEnd)
  }

  const viewModes = allowedViewModes ?? (columnsContent ? (['icons', 'list', 'columns'] as FinderViewMode[]) : (['icons', 'list'] as FinderViewMode[]))
  const showViewModes = viewModes.length > 1

  return (
    <div className="mac-finder flex flex-col -mx-1">
      <div className="mac-finder-toolbar tahoe-toolbar flex flex-wrap items-center gap-2 px-1 sm:px-2 py-2 mx-1 sm:mx-2 mt-2">
        <div className="flex items-center gap-1">
          <Tooltip label="Back"><button type="button" className="mac-finder-nav-btn" onClick={handleBack} title="Back" aria-label="Back">
            <ChevronLeft className="h-4 w-4" />
          </button></Tooltip>
          <Tooltip label="Forward"><button type="button" className="mac-finder-nav-btn" onClick={handleForward} title="Forward" aria-label="Forward">
            <ChevronRight className="h-4 w-4" />
          </button></Tooltip>
        </div>

        {title ? <span className="text-sm font-medium text-[var(--text-primary)] hidden sm:inline">{title}</span> : null}
        {pathSegments && pathSegments.length ? (
          <div className="hidden md:block min-w-0 flex-1 max-w-md">
            <FinderPathBar segments={pathSegments} />
          </div>
        ) : null}

        <div className="relative flex-1 min-w-[140px] max-w-md">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-[var(--text-muted)]" />
          <input
            type="search"
            aria-label={searchPlaceholder || 'Search'}
            value={search}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder={searchPlaceholder}
            className="mac-finder-search w-full pl-8 pr-3 py-1.5 text-sm rounded-lg text-[var(--text-primary)]"
          />
        </div>

        {showViewModes ? (
        <div className="tahoe-segment flex p-0.5 rounded-lg shrink-0">
          {viewModes.includes('icons') ? (
          <button
            type="button"
            onClick={() => onViewModeChange('icons')}
            className={`tahoe-segment-item p-1.5 rounded-md ${viewMode === 'icons' ? 'tahoe-segment-active' : 'text-[var(--text-muted)]'}`}
            title="Icon view"
          >
            <LayoutGrid className="h-4 w-4" />
          </button>
          ) : null}
          {viewModes.includes('list') ? (
          <button
            type="button"
            onClick={() => onViewModeChange('list')}
            className={`tahoe-segment-item p-1.5 rounded-md ${viewMode === 'list' ? 'tahoe-segment-active' : 'text-[var(--text-muted)]'}`}
            title="List view"
          >
            <List className="h-4 w-4" />
          </button>
          ) : null}
          {viewModes.includes('columns') && columnsContent ? (
            <button
              type="button"
              onClick={() => onViewModeChange('columns')}
              className={`tahoe-segment-item p-1.5 rounded-md ${viewMode === 'columns' ? 'tahoe-segment-active' : 'text-[var(--text-muted)]'}`}
              title="Column view"
            >
              <Columns3 className="h-4 w-4" />
            </button>
          ) : null}
        </div>
        ) : null}

        {!isPopout ? (
          <button
            type="button"
            onClick={() => openCenterPopout(`${location.pathname}${location.search}`)}
            className="mac-finder-nav-btn shrink-0 hidden sm:flex items-center gap-1 px-2 w-auto whitespace-nowrap text-xs"
            style={{ width: 'auto' }}
            title="Move to new window (⌘⌥N)"
          >
            <ExternalLink className="h-3.5 w-3.5" />
            <span className="hidden lg:inline">Pop out</span>
          </button>
        ) : null}

        {toolbarActions ? <div className="flex items-center gap-2 shrink-0">{toolbarActions}</div> : null}
      </div>

      <div className="mac-finder-panes flex">
        {isColumns ? (
          <div className="mac-finder-columns flex flex-1 min-w-0">
            {isEmpty && emptyState ? (
              <div className="flex flex-1 items-center justify-center p-6 sm:p-10">{emptyState}</div>
            ) : (
              columnsContent
            )}
          </div>
        ) : (
          <div className="mac-finder-list flex-1 min-w-0 p-2 sm:p-3">
            {isEmpty && emptyState ? emptyState : listContent}
          </div>
        )}

        {showInspectorPane ? (
          <>
            <button
              type="button"
              aria-label="Resize inspector"
              onMouseDown={startResize}
              className="mac-finder-split-handle hidden lg:block w-1 shrink-0 cursor-col-resize hover:bg-[var(--accent)]/40 transition-colors"
            />
            <aside
              className="mac-finder-inspector hidden lg:flex flex-col min-w-0 border-l border-white/[0.06] shrink-0 tahoe-glass-card rounded-none border-y-0 border-r-0"
              style={{ width: inspectorWidth }}
            >
              {inspector}
            </aside>
          </>
        ) : null}
      </div>
    </div>
  )
}
