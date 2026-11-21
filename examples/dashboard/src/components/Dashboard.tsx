"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  burnRate,
  ensureConnected,
  formatAmount,
  loadEmployeeStreams,
  loadEmployerStreams,
  shortAddr,
  STREAM_CONTRACT_ID,
  withdraw,
  withdrawAll,
  type StreamRow,
} from "@/lib/stellar";

type Tab = "employer" | "employee";

type HistoryEventKind = "withdraw" | "topup" | "rate_change" | "status_change";

type HistoryEvent = {
  id: string;
  kind: HistoryEventKind;
  timestamp: number;
  amount?: string;
  rate?: string;
  status?: string;
  txHash?: string;
};

const INDEXER_URL = process.env.NEXT_PUBLIC_INDEXER_URL ?? "";

const EVENT_LABELS: Record<HistoryEventKind, string> = {
  withdraw: "Withdrawal",
  topup: "Top-up",
  rate_change: "Rate change",
  status_change: "Status change",
};

function normalizeEvent(raw: Record<string, unknown>, fallbackId: string): HistoryEvent {
  const rawKind = String(raw.kind ?? raw.type ?? raw.event_type ?? "").toLowerCase();
  let kind: HistoryEventKind = "status_change";
  if (rawKind.includes("withdraw")) kind = "withdraw";
  else if (rawKind.includes("top") || rawKind.includes("deposit") || rawKind.includes("fund")) kind = "topup";
  else if (rawKind.includes("rate")) kind = "rate_change";
  else if (rawKind.includes("status") || rawKind.includes("pause") || rawKind.includes("resume") || rawKind.includes("cancel") || rawKind.includes("complete")) kind = "status_change";

  const tsRaw = raw.timestamp ?? raw.ledger_time ?? raw.created_at ?? raw.time ?? 0;
  const timestamp =
    typeof tsRaw === "number"
      ? tsRaw
      : Number.isFinite(Date.parse(String(tsRaw)))
        ? Date.parse(String(tsRaw))
        : Number(tsRaw) || 0;

  return {
    id: String(raw.id ?? raw.event_id ?? raw.tx_hash ?? fallbackId),
    kind,
    timestamp,
    amount: raw.amount != null ? String(raw.amount) : undefined,
    rate: raw.rate != null ? String(raw.rate) : raw.rate_per_second != null ? String(raw.rate_per_second) : undefined,
    status: raw.status != null ? String(raw.status) : raw.status_label != null ? String(raw.status_label) : undefined,
    txHash: raw.tx_hash != null ? String(raw.tx_hash) : raw.transaction_hash != null ? String(raw.transaction_hash) : undefined,
  };
}

async function fetchStreamHistory(streamId: bigint): Promise<HistoryEvent[]> {
  if (!INDEXER_URL) throw new Error("Indexer URL not configured");
  const res = await fetch(`${INDEXER_URL.replace(/\/$/, "")}/streams/${streamId.toString()}/events`);
  if (!res.ok) throw new Error(`Indexer responded ${res.status}`);
  const data = await res.json();
  const list: unknown[] = Array.isArray(data) ? data : Array.isArray(data?.events) ? data.events : [];
  return list
    .map((item, i) => normalizeEvent(item as Record<string, unknown>, `evt-${i}`))
    .sort((a, b) => a.timestamp - b.timestamp);
}

function formatEventTime(ts: number): string {
  if (!ts) return "—";
  const ms = ts < 1e12 ? ts * 1000 : ts;
  return new Date(ms).toLocaleString();
}

function downloadFile(filename: string, content: string, mime: string) {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

function eventsToCsv(events: HistoryEvent[]): string {
  const header = ["timestamp", "kind", "amount", "rate", "status", "tx_hash"];
  const rows = events.map((e) =>
    [formatEventTime(e.timestamp), e.kind, e.amount ?? "", e.rate ?? "", e.status ?? "", e.txHash ?? ""]
      .map((v) => `"${String(v).replace(/"/g, '""')}"`)
      .join(","),
  );
  return [header.join(","), ...rows].join("\n");
}

export default function Dashboard() {
  const [address, setAddress] = useState("");
  const [tab, setTab] = useState<Tab>("employer");
  const [employerStreams, setEmployerStreams] = useState<StreamRow[]>([]);
  const [employeeStreams, setEmployeeStreams] = useState<StreamRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [selectedStreamId, setSelectedStreamId] = useState<string | null>(null);
  const [history, setHistory] = useState<HistoryEvent[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState("");
  const [tick, setTick] = useState(0);

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
    }
  }, []);

  useEffect(() => {
    const interval = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(interval);
  }, []);

  const loadHistory = useCallback(async (streamId: string) => {
    setHistoryLoading(true);
    setHistoryError("");
    try {
      const events = await fetchStreamHistory(BigInt(streamId));
      setHistory(events);
    } catch (e) {
      setHistory([]);
      setHistoryError(String((e as Error).message ?? e));
    } finally {
      setHistoryLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!selectedStreamId) {
      setHistory([]);
      setHistoryError("");
      return;
    }
    void loadHistory(selectedStreamId);
  }, [selectedStreamId, loadHistory]);

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

  const selectedStream = useMemo(
    () => streams.find((s) => s.id.toString() === selectedStreamId) ?? null,
    [streams, selectedStreamId],
  );

  const withdrawnToDate = useMemo(
    () =>
      history
        .filter((e) => e.kind === "withdraw" && e.amount)
        .reduce((sum, e) => sum + BigInt(Math.trunc(Number(e.amount))), 0n),
    [history],
  );

  const projectedMonthly = useMemo(() => {
    if (!selectedStream || selectedStream.status_label !== "Active") return 0n;
    const now = BigInt(Math.floor(Date.now() / 1000));
    const stop = selectedStream.stop_time ?? 0n;
    const secondsLeft = stop > now ? stop - now : 0n;
    const monthSeconds = 30n * 24n * 60n * 60n;
    const cappedSeconds = secondsLeft < monthSeconds ? secondsLeft : monthSeconds;
    return selectedStream.rate_per_second * cappedSeconds;
  }, [selectedStream, tick]);

  return (
    <main>
      <header>
        <div>
          <h1>PayStream Dashboard</h1>
          <div className="muted">
            contract <code>{STREAM_CONTRACT_ID ? shortAddr(STREAM_CONTRACT_ID) : "unset"}</code> · Stellar testnet
          </div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          {address && (
            <button className="secondary" onClick={() => refresh(address)} disabled={loading}>
              {loading ? "Refreshing…" : "Refresh"}
            </button>
          )}
          <button onClick={connect} disabled={loading}>
            {address ? shortAddr(address) : "Connect Freighter"}
          </button>
        </div>
      </header>

      {error && <div className="error">{error}</div>}

      {!address ? (
        <div className="notice">
          Connect your Freighter wallet to view streams. The dashboard reads streams where the
          connected account is employer or employee on the configured testnet contract.
        </div>
      ) : (
        <>
          <div className="tabs">
            <button className={tab === "employer" ? "active" : ""} onClick={() => setTab("employer")}>
              Employer ({employerStreams.length})
            </button>
            <button className={tab === "employee" ? "active" : ""} onClick={() => setTab("employee")}>
              Employee ({employeeStreams.length})
            </button>
          </div>

          {tab === "employer" ? (
            <div className="cards">
              <Stat label="Streams" value={String(employerStreams.length)} />
              <Stat label="Burn rate" value={`${formatAmount(burnRate(employerStreams))}/s`} />
              <Stat label="Total claimable" value={formatAmount(totalClaimable)} />
              <Stat
                label="Total remaining"
                value={formatAmount(employerStreams.reduce((s, x) => s + x.remaining, 0n))}
              />
            </div>
          ) : (
            <>
              <div className="cards">
                <Stat label="Streams" value={String(employeeStreams.length)} />
                <Stat label="Claimable now" value={formatAmount(totalClaimable)} />
              </div>
              <EarningsSummary
                streams={employeeStreams}
                claimableNow={totalClaimable}
                withdrawnToDate={withdrawnToDate}
                projectedMonthly={projectedMonthly}
                hasSelection={selectedStream !== null}
              />
            </>
          )}

          {tab === "employee" && totalClaimable > 0n && (
            <p>
              <button onClick={doWithdrawAll} disabled={busyId !== null}>
                {busyId === "all" ? "Withdrawing…" : "Withdraw all"}
              </button>
            </p>
          )}

          {streams.length === 0 ? (
            <div className="notice">No {tab} streams for this account.</div>
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
  return (
    <table>
      <thead>
        <tr>
          <th>ID</th>
          <th>{tab === "employer" ? "Employee" : "Employer"}</th>
          <th>Status</th>
          <th>Claimable</th>
          <th>Burn rate</th>
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
              <span className={`pill ${s.status_label}`}>{s.status_label}</span>
            </td>
            <td>{formatAmount(s.claimable)}</td>
            <td>{s.status_label === "Active" ? `${formatAmount(s.rate_per_second)}/s` : "—"}</td>
            {tab === "employee" && (
              <td style={{ textAlign: "right" }}>
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
  );
}
