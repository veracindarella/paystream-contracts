"use client";

import { useCallback, useEffect, useState } from "react";
import { useLocale, useTranslations } from "next-intl";
import {
  burnRate,
  ensureConnected,
  formatAmount,
  getMinDeposit,
  getPaused,
  loadEmployeeStreams,
  loadEmployerStreams,
  NETWORK,
  shortAddr,
  STREAM_CONTRACT_ID,
  withdraw,
  withdrawAll,
  type StreamRow,
} from "@/lib/stellar";
import { formatAmount, formatRate } from "@/lib/format";

type Tab = "employer" | "employee";

const STORAGE_KEY = "paystream.session";
const LOW_BALANCE_DISMISS_KEY = "paystream.lowBalanceDismissed";

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Reusable focus-trap hook for modals/dialogs.
 *
 * While `active` is true, Tab/Shift+Tab are constrained to the focusable
 * elements inside `containerRef`, and focus is moved into the container.
 * When the trap deactivates, focus is restored to the element that was
 * focused before the trap engaged.
 */
export function useFocusTrap(
  containerRef: React.RefObject<HTMLElement | null>,
  active: boolean,
) {
  useEffect(() => {
    if (!active) return;
    const container = containerRef.current;
    if (!container) return;

    const previouslyFocused = document.activeElement as HTMLElement | null;

    const getFocusable = () =>
      Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
        (el) => el.offsetParent !== null || el === document.activeElement,
      );

    const focusables = getFocusable();
    (focusables[0] ?? container).focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = getFocusable();
      if (items.length === 0) {
        event.preventDefault();
        container.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const current = document.activeElement as HTMLElement | null;

      if (event.shiftKey) {
        if (current === first || !container.contains(current)) {
          event.preventDefault();
          last.focus();
        }
      } else if (current === last || !container.contains(current)) {
        event.preventDefault();
        first.focus();
      }
    };

    container.addEventListener("keydown", onKeyDown);
    return () => {
      container.removeEventListener("keydown", onKeyDown);
      if (previouslyFocused && typeof previouslyFocused.focus === "function") {
        previouslyFocused.focus();
      }
    };
  }, [containerRef, active]);
}

type PersistedSession = { address: string; tab: Tab };

function readPersistedSession(): PersistedSession | null {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<PersistedSession>;
    if (typeof parsed.address !== "string" || !parsed.address) return null;
    const tab: Tab = parsed.tab === "employee" ? "employee" : "employer";
    return { address: parsed.address, tab };
  } catch {
    return null;
  }
}

function clearPersistedSession() {
  try {
    window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
}

function writePersistedSession(session: PersistedSession) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(session));
  } catch {
    /* ignore */
  }
}

export default function Dashboard() {
  const t = useTranslations("Dashboard");
  const locale = useLocale();
  const [address, setAddress] = useState("");
  const [tab, setTab] = useState<Tab>("employer");
  const [employerStreams, setEmployerStreams] = useState<StreamRow[]>([]);
  const [employeeStreams, setEmployeeStreams] = useState<StreamRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [minDeposit, setMinDeposit] = useState<bigint | null>(null);
  const [paused, setPaused] = useState<boolean | null>(null);
  const [deposit, setDeposit] = useState("");
  const [creating, setCreating] = useState(false);

  const refresh = useCallback(async (addr: string) => {
    setLoading(true);
    setError("");
    try {
      const [emp, epi] = await Promise.all([
        loadEmployerStreams(addr),
        loadEmployeeStreams(addr),
      ]);
      setEmployerStreams(emp);
      setEmployeeStreams(epi);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setLoading(false);
    }
  }, []);

  const connect = async () => {
    try {
      const addr = await ensureConnected();
      setAddress(addr);
      await refresh(addr);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    }
  };

  useEffect(() => {
    if (!STREAM_CONTRACT_ID) {
      setError("NEXT_PUBLIC_STREAM_CONTRACT_ID is not set — copy .env.example to .env.local.");
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const [min, isPaused] = await Promise.all([getMinDeposit(), getPaused()]);
        if (!cancelled) {
          setMinDeposit(min);
          setPaused(isPaused);
        }
      } catch (e) {
        if (!cancelled) setError(String((e as Error).message ?? e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const depositAmount = (() => {
    try {
      return deposit.trim() === "" ? null : BigInt(deposit.trim());
    } catch {
      return null;
    }
  })();

  const depositTooLow =
    minDeposit !== null && (depositAmount === null || depositAmount < minDeposit);

  const createDisabled =
    creating || paused !== true ? creating || paused === null : true;

  const doWithdraw = async (streamId: bigint) => {
    setBusyId(streamId.toString());
    setError("");
    try {
      await withdraw(address, streamId);
      await refresh(address);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusyId(null);
    }
  };

  const doCreate = async () => {
    if (depositAmount === null) return;
    setCreating(true);
    setError("");
    try {
      // Re-validate against the contract before submitting.
      const [min, isPaused] = await Promise.all([getMinDeposit(), getPaused()]);
      setMinDeposit(min);
      setPaused(isPaused);
      if (isPaused) {
        setError("Contract is paused — new streams cannot be created right now.");
        return;
      }
      if (depositAmount < min) {
        setError(`Deposit is below the minimum of ${formatAmount(min)}.`);
        return;
      }
      // create_stream submission is handled by the create form flow.
      await refresh(address);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setCreating(false);
    }
  };

  const doWithdrawAll = async () => {
    setBusyId("all");
    setError("");
    try {
      await withdrawAll(address);
      await refresh(address);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusyId(null);
    }
  };

  const streams = tab === "employer" ? employerStreams : employeeStreams;
  const totalClaimable = streams
    .filter((s) => s.status_label === "Active")
    .reduce((sum, s) => sum + s.claimable, 0n);

  const lowBalanceStreams = employerStreams.filter(
    (s) => s.status_label === "Active" && s.remaining < s.low_balance_threshold,
  );

  return (
    <main id="main-content">
      <a href="#main-content" className="skip-link">
        Skip to content
      </a>
      <header>
        <div>
          <h1>{t("title")}</h1>
          <div className="muted">
            {t("contractLabel")}{" "}
            <code>{STREAM_CONTRACT_ID ? shortAddr(STREAM_CONTRACT_ID) : t("contractUnset")}</code>{" "}
            · {t("network")}
          </div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          {address && (
            <button className="secondary" onClick={() => refresh(address)} disabled={loading}>
              {loading ? t("refreshing") : t("refresh")}
            </button>
          )}
          <button onClick={connect} disabled={loading}>
            {address ? shortAddr(address) : t("connect")}
          </button>
        </div>
      </header>

      {error && <div className="error">{error}</div>}

      {paused === true && (
        <div className="notice">
          The contract is currently paused — new streams cannot be created.
        </div>
      )}

      {STREAM_CONTRACT_ID && (
        <div className="card" style={{ marginBottom: 16 }}>
          <div className="label">Create stream</div>
          <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 8 }}>
            <input
              type="text"
              inputMode="numeric"
              placeholder="Deposit amount"
              value={deposit}
              onChange={(e) => setDeposit(e.target.value)}
              disabled={creating}
            />
            <button onClick={doCreate} disabled={createDisabled || depositTooLow}>
              {creating ? "Creating…" : "Create stream"}
            </button>
          </div>
          {minDeposit !== null && (
            <div className="muted" style={{ marginTop: 8 }}>
              Minimum deposit: {formatAmount(minDeposit)}
            </div>
          )}
          {depositTooLow && deposit.trim() !== "" && (
            <div className="muted" style={{ marginTop: 4 }}>
              Deposit must be at least {minDeposit !== null ? formatAmount(minDeposit) : "the minimum"}.
            </div>
          )}
        </div>
      )}

      {!address ? (
        <div className="notice">{t("connectNotice")}</div>
      ) : (
        <>
          <div className="tabs">
            <button className={tab === "employer" ? "active" : ""} onClick={() => setTab("employer")}>
              {t("tabs.employer", { count: employerStreams.length })}
            </button>
            <button className={tab === "employee" ? "active" : ""} onClick={() => setTab("employee")}>
              {t("tabs.employee", { count: employeeStreams.length })}
            </button>
          </div>

          {tab === "employer" ? (
            <div className="cards">
              <Stat label={t("stats.streams")} value={String(employerStreams.length)} />
              <Stat
                label={t("stats.burnRate")}
                value={formatRate(burnRate(employerStreams), locale)}
              />
              <Stat label={t("stats.totalClaimable")} value={formatAmount(totalClaimable, locale)} />
              <Stat
                label={t("stats.totalRemaining")}
                value={formatAmount(
                  employerStreams.reduce((s, x) => s + x.remaining, 0n),
                  locale,
                )}
              />
            </div>
          ) : (
            <div className="cards">
              <Stat label={t("stats.streams")} value={String(employeeStreams.length)} />
              <Stat label={t("stats.claimableNow")} value={formatAmount(totalClaimable, locale)} />
            </div>
          )}

          {tab === "employee" && totalClaimable > 0n && (
            <p>
              <button onClick={doWithdrawAll} disabled={busyId !== null}>
                {busyId === "all" ? t("withdrawing") : t("withdrawAll")}
              </button>
            </p>
          )}

          {streams.length === 0 ? (
            <div className="notice">{t("empty", { tab: t(`tabNames.${tab}`) })}</div>
          ) : (
            <>
              <StreamTable
                tab={tab}
                streams={streams}
                busyId={busyId}
                selectedId={selectedStreamId}
                onSelect={setSelectedStreamId}
                onWithdraw={doWithdraw}
              />
              <TransactionHistory
                streamId={selectedStreamId}
                events={history}
                loading={historyLoading}
                error={historyError}
                onRetry={() => selectedStreamId && loadHistory(selectedStreamId)}
              />
            </>
          )}
        </>
      )}
    </main>
  );
}

function EarningsSummary({
  streams,
  claimableNow,
  withdrawnToDate,
  projectedMonthly,
  hasSelection,
}: {
  streams: StreamRow[];
  claimableNow: bigint;
  withdrawnToDate: bigint;
  projectedMonthly: bigint;
  hasSelection: boolean;
}) {
  if (streams.length === 0) {
    return (
      <div className="notice">
        No employee streams yet — earnings will appear here once you are added to a stream.
      </div>
    );
  }
  return (
    <div className="cards">
      <Stat label="Claimable now" value={formatAmount(claimableNow)} />
      <Stat label="Withdrawn to date" value={formatAmount(withdrawnToDate)} />
      <Stat
        label="Projected monthly"
        value={hasSelection ? formatAmount(projectedMonthly) : "Select a stream"}
      />
    </div>
  );
}

function TransactionHistory({
  streamId,
  events,
  loading,
  error,
  onRetry,
}: {
  streamId: string | null;
  events: HistoryEvent[];
  loading: boolean;
  error: string;
  onRetry: () => void;
}) {
  if (!streamId) {
    return (
      <div className="notice">Select a stream to view its transaction history.</div>
    );
  }

  const exportCsv = () =>
    downloadFile(`stream-${streamId}-history.csv`, eventsToCsv(events), "text/csv");
  const exportJson = () =>
    downloadFile(
      `stream-${streamId}-history.json`,
      JSON.stringify(events, null, 2),
      "application/json",
    );

  return (
    <section style={{ marginTop: 24 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h2 style={{ fontSize: 16 }}>Transaction history · stream {streamId}</h2>
        <div style={{ display: "flex", gap: 8 }}>
          <button className="secondary" onClick={exportCsv} disabled={events.length === 0}>
            Export CSV
          </button>
          <button className="secondary" onClick={exportJson} disabled={events.length === 0}>
            Export JSON
          </button>
        </div>
      </div>

      {loading ? (
        <div className="notice">Loading indexed events…</div>
      ) : error ? (
        <div className="notice">
          Transaction history is unavailable right now ({error}).{" "}
          <button className="secondary" onClick={onRetry}>
            Retry
          </button>
        </div>
      ) : events.length === 0 ? (
        <div className="notice">No indexed events for this stream yet.</div>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Time</th>
              <th>Event</th>
              <th>Details</th>
              <th>Tx</th>
            </tr>
          </thead>
          <tbody>
            {events.map((e) => (
              <tr key={e.id}>
                <td>{formatEventTime(e.timestamp)}</td>
                <td>{EVENT_LABELS[e.kind]}</td>
                <td>
                  {e.kind === "withdraw" && e.amount ? formatAmount(BigInt(Math.trunc(Number(e.amount)))) : null}
                  {e.kind === "topup" && e.amount ? `+${formatAmount(BigInt(Math.trunc(Number(e.amount))))}` : null}
                  {e.kind === "rate_change" && e.rate ? `${formatAmount(BigInt(Math.trunc(Number(e.rate))))}/s` : null}
                  {e.kind === "status_change" && e.status ? e.status : null}
                  {!e.amount && !e.rate && !e.status ? "—" : null}
                </td>
                <td>{e.txHash ? <code>{shortAddr(e.txHash)}</code> : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="card">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
    </div>
  );
}

function StreamTable({
  tab,
  streams,
  busyId,
  selectedId,
  onSelect,
  onWithdraw,
}: {
  tab: Tab;
  streams: StreamRow[];
  busyId: string | null;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onWithdraw: (id: bigint) => void;
}) {
  const t = useTranslations("Dashboard");
  const locale = useLocale();
  return (
    <table>
      <thead>
        <tr>
          <th>{t("table.id")}</th>
          <th>{tab === "employer" ? t("table.employee") : t("table.employer")}</th>
          <th>{t("table.status")}</th>
          <th>{t("table.claimable")}</th>
          <th>{t("table.burnRate")}</th>
          {tab === "employee" && <th />}
        </tr>
      </thead>
      <tbody>
        {streams.map((s) => (
          <tr
            key={s.id.toString()}
            onClick={() => onSelect(s.id.toString())}
            style={{
              cursor: "pointer",
              background: selectedId === s.id.toString() ? "rgba(0,0,0,0.05)" : undefined,
            }}
          >
            <td>{s.id.toString()}</td>
            <td>
              <code>{shortAddr(tab === "employer" ? s.employee : s.employer)}</code>
            </td>
            <td>
              <span className={`pill ${s.status_label}`}>
                {t.has(`status.${s.status_label}`)
                  ? t(`status.${s.status_label}`)
                  : s.status_label}
              </span>
            </td>
            <td>{formatAmount(s.claimable, locale)}</td>
            <td>
              {s.status_label === "Active"
                ? formatRate(s.rate_per_second, locale)
                : t("table.dash")}
            </td>
            {tab === "employee" && (
              <td style={{ textAlign: "right" }}>
                <button
                  onClick={() => onWithdraw(s.id)}
                  disabled={s.status_label !== "Active" || s.claimable <= 0n || busyId !== null}
                >
                  {busyId === s.id.toString() ? t("table.busy") : t("table.withdraw")}
                </button>
              </td>
            )}
          </tr>
        </thead>
        <tbody>
          {streams.map((s) => (
            <tr key={s.id.toString()} id={`stream-${s.id.toString()}`}>
              <td data-label="ID" className="sticky-col">
                {s.id.toString()}
              </td>
              <td data-label={tab === "employer" ? "Employee" : "Employer"}>
                <code>{shortAddr(tab === "employer" ? s.employee : s.employer)}</code>
              </td>
              <td data-label="Status">
                <span className={`pill ${s.status_label}`}>{s.status_label}</span>
              </td>
              <td data-label="Claimable">{formatAmount(s.claimable)}</td>
              <td data-label="Burn rate">
                {s.status_label === "Active" ? `${formatAmount(s.rate_per_second)}/s` : "—"}
              </td>
              {tab === "employee" && (
                <td data-label="Actions" style={{ textAlign: "right" }}>
                  <button
                    onClick={() => onWithdraw(s.id)}
                    disabled={s.status_label !== "Active" || s.claimable <= 0n || busyId !== null}
                  >
                    {busyId === s.id.toString() ? "…" : "Withdraw"}
                  </button>
                </td>
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
