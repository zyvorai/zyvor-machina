// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { motion, type HTMLMotionProps } from 'framer-motion'

const spring = { type: 'spring' as const, stiffness: 300, damping: 25, mass: 0.8 }

type GlassCardProps = HTMLMotionProps<'div'> & {
  elevated?: boolean
  strong?: boolean
  hover?: boolean
}

/** Zeus flat premium card — hairline + surface, no content blur/lift. */
export function GlassCard({
  elevated: _elevated = true,
  strong: _strong = false,
  hover = false,
  className = '',
  children,
  ...props
}: GlassCardProps) {
  return (
    <motion.div
      whileHover={hover ? { scale: 1.002 } : undefined}
      transition={spring}
      className={`tahoe-glass-card rounded-[var(--radius-card,0.9rem)] ${className}`.trim()}
      {...props}
    >
      {children}
    </motion.div>
  )
}
