import { motion } from "framer-motion"
import type { ReactNode } from "react"

export const cardVariants = {
  hidden: { opacity: 0, y: 18, scale: 0.98 },
  show: { opacity: 1, y: 0, scale: 1 },
}

export function PageChrome({ icon, eyebrow, title, description, actions, children }: { icon: ReactNode; eyebrow: string; title: string; description?: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <div className="react-page">
      <motion.section className="react-hero react-page-hero" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.42 }}>
        <div className="react-hero-grid" aria-hidden="true" />
        <div className="react-hero-copy">
          <span className="react-eyebrow">{icon} {eyebrow}</span>
          <h1>{title}</h1>
          {description ? <p>{description}</p> : null}
          {actions ? <div className="react-hero-actions">{actions}</div> : null}
        </div>
      </motion.section>
      {children}
    </div>
  )
}

export function LoadingCard({ label = "正在加载…" }: { label?: string }) { return <div className="react-loading">{label}</div> }
export function ErrorCard({ error }: { error: string }) { return <div className="react-error">加载失败：{error}</div> }
export function EmptyCard({ children }: { children: ReactNode }) { return <div className="react-empty">{children}</div> }

export function PanelHead({ kicker, title, chip }: { kicker: string; title: string; chip?: ReactNode }) {
  return <div className="react-panel-head"><div><span>{kicker}</span><h2>{title}</h2></div>{chip ? <em>{chip}</em> : null}</div>
}

export function KpiCard({ icon, label, value, sub, tone }: { icon: ReactNode; label: string; value: string | number; sub: string; tone: string }) {
  return <motion.article className={`react-kpi react-kpi-${tone}`} variants={cardVariants} whileHover={{ y: -5, scale: 1.01 }} transition={{ type: "spring", stiffness: 260, damping: 24 }}><div className="react-kpi-icon">{icon}</div><span className="react-kpi-label">{label}</span><motion.strong className="react-kpi-value" layout>{value}</motion.strong><span className="react-kpi-sub">{sub}</span></motion.article>
}
