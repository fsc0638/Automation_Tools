"use client";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { ArrowLeft, Upload } from "lucide-react";
import { MeetingSidebar } from "@/components/meetings/MeetingSidebar";
import { WeeklyMiniCalendar } from "@/components/meetings/WeeklyMiniCalendar";
import { TimeSlotPanel } from "@/components/meetings/TimeSlotPanel";
import {
  meetings as meetingsApi,
  portalDirectory as portalDirectoryApi,
  projects as projectsApi,
  type MeetingImportance,
  type MeetingRecurrence,
  type MeetingTimeSlot,
  type PortalDepartment,
  type PortalEmployee,
  type RoomAvailability,
} from "@/lib/api";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

// Native HTML5 date / time inputs expect ISO formats: "YYYY-MM-DD" for
// date, "HH:mm" (24h) for time. We standardize on those here so the form
// can use the browser's date picker / time picker instead of free-text.
function todayDateInput(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function defaultTime(hour: number, minute: number): string {
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}

function parseDateTime(dateStr: string, timeStr: string): Date | null {
  const m = dateStr.match(/^(\d{4})-(\d{1,2})-(\d{1,2})$/);
  const tMatch = timeStr.match(/^(\d{1,2}):(\d{2})$/);
  if (!m || !tMatch) return null;
  const hour = parseInt(tMatch[1], 10);
  const minute = parseInt(tMatch[2], 10);
  return new Date(parseInt(m[1]), parseInt(m[2]) - 1, parseInt(m[3]), hour, minute);
}

export default function NewMeetingPage() {
  const t = useT();
  const router = useRouter();
  const sp = useSearchParams();
  const initialProjectId = sp.get("project_id") ?? null;

  const [title, setTitle] = useState("產品週會 · Sprint review");
  const [importance, setImportance] = useState<MeetingImportance>("important");
  const [startDate, setStartDate] = useState(todayDateInput());
  const [startTime, setStartTime] = useState(defaultTime(9, 30));
  const [endTime, setEndTime] = useState(defaultTime(10, 30));
  const [endDate, setEndDate] = useState(todayDateInput());
  const [allDay, setAllDay] = useState(false);
  const [recurrence, setRecurrence] = useState<MeetingRecurrence>("none");
  const [timezone, setTimezone] = useState("Asia/Taipei");
  // Location: 實體 vs 線上 toggle. In 實體 mode `location` is a room name
  // chosen from the rooms-available endpoint; in 線上 mode it's the provider
  // label ("Webex" / "Microsoft Teams" / "Google Meet"). Actual link
  // generation is deferred — for now the user just records which provider.
  const [locationMode, setLocationMode] = useState<"physical" | "online">("physical");
  const [location, setLocation] = useState("");
  const [onlineProvider, setOnlineProvider] = useState<"webex" | "teams" | "meet">("meet");
  const [rooms, setRooms] = useState<RoomAvailability[]>([]);
  const [roomsLoading, setRoomsLoading] = useState(false);

  // Attendees: free-text field as source of truth (semicolon-separated). The
  // last token before the caret feeds the employee-search autocomplete; when
  // the user picks a row we replace that token with the employee's name +
  // ";". A side map keeps name→email so the submit step can convert.
  const [attendees, setAttendees] = useState("");
  const [attendeeDept, setAttendeeDept] = useState<string>("");
  const [departments, setDepartments] = useState<PortalDepartment[]>([]);
  const [employeeMatches, setEmployeeMatches] = useState<PortalEmployee[]>([]);
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [pickedEmails, setPickedEmails] = useState<Record<string, string>>({});
  const [notificationNote, setNotificationNote] = useState("請先檢閱附件並備妥議題。");
  // AgentK-aligned: description is the long-form meeting介紹 (vs.
  // notification_note which is the invitation message). join_url is the
  // online-meeting link; when locationMode=='online' we'll populate it
  // from the provider auto-link once that integration lands.
  const [description, setDescription] = useState("");
  const [joinUrl, setJoinUrl] = useState("");
  const [projectId, setProjectId] = useState<string | null>(initialProjectId);
  const [projectName, setProjectName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [files, setFiles] = useState<Array<{ name: string; status: "attached" | "pending" }>>([
    { name: "meeting-prep-pack.pdf", status: "attached" },
    { name: "q2-risk-register.xlsx", status: "pending" },
    { name: "last-review-minutes.docx", status: "attached" },
  ]);
  const [slots, setSlots] = useState<MeetingTimeSlot[]>([]);
  const [selectedSlotStart, setSelectedSlotStart] = useState<string | null>(null);

  // Available projects for the linkage dropdown. Empty array until the
  // first fetch resolves. Without this list the user could only link a
  // meeting via `?project_id=...` in the URL, which made the "同步成任務"
  // button on the detail page always say "未連結至專案".
  const [allProjects, setAllProjects] = useState<Array<{ id: string; name: string }>>([]);
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await projectsApi.list();
        if (!cancelled) setAllProjects(list.map((p) => ({ id: p.id, name: p.name })));
      } catch {
        if (!cancelled) setAllProjects([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Hydrate linked project name when we arrived from a project page.
  useEffect(() => {
    if (!projectId) {
      setProjectName(null);
      return;
    }
    (async () => {
      try {
        const p = await projectsApi.get(projectId);
        setProjectName(p.name);
      } catch {
        setProjectName(null);
      }
    })();
  }, [projectId]);

  const startDateObj = useMemo(
    () => parseDateTime(startDate, startTime) ?? new Date(),
    [startDate, startTime]
  );
  const endDateObj = useMemo(
    () => parseDateTime(endDate, endTime) ?? new Date(),
    [endDate, endTime]
  );

  const startIso = useMemo(() => startDateObj.toISOString(), [startDateObj]);
  const endIso = useMemo(() => endDateObj.toISOString(), [endDateObj]);

  // Load departments once for the attendees filter. Static enough that we
  // don't need to refresh; but we expose a manual reload so the dept select
  // can re-try on focus if the first load lost the race with backend boot.
  const loadDepartments = useCallback(async () => {
    try {
      const list = await portalDirectoryApi.departments();
      setDepartments(list);
    } catch {
      setDepartments([]);
    }
  }, []);

  useEffect(() => {
    void loadDepartments();
  }, [loadDepartments]);

  const loadRooms = useCallback(async () => {
    if (endDateObj <= startDateObj) return;
    setRoomsLoading(true);
    try {
      const list = await meetingsApi.roomsAvailable(startIso, endIso);
      setRooms(list);
    } catch {
      setRooms([]);
    } finally {
      setRoomsLoading(false);
    }
  }, [startIso, endIso, startDateObj, endDateObj]);

  // Rooms availability — debounced refresh whenever the time window changes
  // while we're in 實體 mode. Skipped in 線上 mode because we don't need it.
  useEffect(() => {
    if (locationMode !== "physical") return;
    const handle = window.setTimeout(() => {
      void loadRooms();
    }, 400);
    return () => window.clearTimeout(handle);
  }, [locationMode, loadRooms]);

  // Switching to 線上 fills `location` with the provider label so the saved
  // meeting carries something meaningful even before link generation lands.
  useEffect(() => {
    if (locationMode === "online") {
      const label =
        onlineProvider === "webex"
          ? "Webex"
          : onlineProvider === "teams"
            ? "Microsoft Teams"
            : "Google Meet";
      setLocation(label);
    }
  }, [locationMode, onlineProvider]);

  // Attendees autocomplete: extract the last `;`-separated token after the
  // caret as the search fragment. Empty fragment + no dept filter = hide the
  // panel to avoid dumping the whole table on focus.
  const attendeeFragment = useMemo(() => {
    const tail = attendees.split(";").pop() ?? "";
    return tail.trim();
  }, [attendees]);

  useEffect(() => {
    if (!showSuggestions) return;
    if (!attendeeFragment && !attendeeDept) {
      setEmployeeMatches([]);
      return;
    }
    let cancelled = false;
    const handle = window.setTimeout(async () => {
      try {
        const list = await portalDirectoryApi.searchEmployees({
          q: attendeeFragment,
          deptCode: attendeeDept || undefined,
          limit: 15,
        });
        if (!cancelled) setEmployeeMatches(list);
      } catch {
        if (!cancelled) setEmployeeMatches([]);
      }
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [attendeeFragment, attendeeDept, showSuggestions]);

  function pickEmployee(emp: PortalEmployee) {
    // Replace the trailing fragment with the picked name + "; " so the user
    // can keep typing the next attendee. If the field was empty we just
    // prepend.
    setAttendees((prev) => {
      const idx = prev.lastIndexOf(";");
      const head = idx >= 0 ? prev.slice(0, idx + 1) : "";
      const sep = head && !head.endsWith(" ") ? " " : "";
      return `${head}${sep}${emp.name}; `;
    });
    if (emp.email) {
      setPickedEmails((m) => ({ ...m, [emp.name]: emp.email as string }));
    }
    setShowSuggestions(false);
  }

  // Re-fetch recommended slots whenever attendees or selected day changes.
  // 600 ms debounce so typing emails doesn't hammer the backend.
  useEffect(() => {
    let cancelled = false;
    const handle = window.setTimeout(async () => {
      try {
        const emails = attendees
          .split(/[,、;]/)
          .map((s) => s.trim())
          .filter((s) => s.includes("@"));
        const dateStr = `${startDateObj.getFullYear()}-${String(startDateObj.getMonth() + 1).padStart(2, "0")}-${String(startDateObj.getDate()).padStart(2, "0")}`;
        const list = await meetingsApi.availableSlots(dateStr, 60, emails);
        if (!cancelled) setSlots(list);
      } catch {
        if (!cancelled) setSlots([]);
      }
    }, 600);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [attendees, startDateObj]);

  function applySlot(slot: MeetingTimeSlot) {
    const s = new Date(slot.start_at);
    const e = new Date(slot.end_at);
    const fmtTime = (d: Date) =>
      `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
    const fmtDate = (d: Date) =>
      `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    setStartDate(fmtDate(s));
    setEndDate(fmtDate(e));
    setStartTime(fmtTime(s));
    setEndTime(fmtTime(e));
    setSelectedSlotStart(slot.start_at);
  }

  const submit = useCallback(
    async (saveAsDraft: boolean) => {
      setError("");
      const startAt = parseDateTime(startDate, startTime);
      const endAt = parseDateTime(endDate, endTime);
      if (!title.trim()) {
        setError("會議名稱必填");
        return;
      }
      if (!startAt || !endAt) {
        setError("日期或時間格式不正確");
        return;
      }
      if (endAt <= startAt) {
        setError("結束時間需在開始時間之後");
        return;
      }
      // Tokens may be raw emails (free-typed) or display names previously
      // picked from the autocomplete. Names get resolved back to email via
      // pickedEmails; tokens that look neither like emails nor like picked
      // names get surfaced to the user — silently dropping them led to
      // "為什麼少了一個人" support tickets.
      const tokens = attendees
        .split(/[,、;]/)
        .map((s) => s.trim())
        .filter((s) => s.length > 0);
      const unresolved: string[] = [];
      const attendee_emails: string[] = [];
      for (const tok of tokens) {
        if (tok.includes("@")) {
          attendee_emails.push(tok);
        } else if (pickedEmails[tok]) {
          attendee_emails.push(pickedEmails[tok]);
        } else {
          unresolved.push(tok);
        }
      }
      if (unresolved.length > 0) {
        setError(
          `以下與會人無法對應到 email，請從建議清單中重新選擇或改填完整 email：${unresolved.join("、")}`
        );
        return;
      }

      setBusy(true);
      try {
        // AgentK-aligned: when locationMode=='online' we record the
        // provider symbolically AND pass the provider's URL into
        // join_url. The on-page `location` field still gets the human-
        // readable label so existing list/calendar views keep rendering.
        const providerSymbol =
          locationMode === "online"
            ? onlineProvider === "webex"
              ? "webex"
              : onlineProvider === "teams"
                ? "teams"
                : "meet"
            : null;
        const created = await meetingsApi.create({
          title: title.trim(),
          importance,
          start_at: startAt.toISOString(),
          end_at: endAt.toISOString(),
          all_day: allDay,
          recurrence,
          timezone,
          location: location.trim() || null,
          notification_note: notificationNote.trim() || null,
          attendee_emails,
          project_id: projectId,
          save_as_draft: saveAsDraft,
          description: description.trim() || null,
          join_url: joinUrl.trim() || null,
          external_provider: providerSymbol,
        });
        router.push(`/meetings/${created.id}`);
      } catch (e) {
        setError(e instanceof Error ? e.message : "建立失敗");
        setBusy(false);
      }
    },
    [title, importance, startDate, startTime, endDate, endTime, allDay, recurrence, timezone, location, locationMode, onlineProvider, attendees, pickedEmails, notificationNote, description, joinUrl, projectId, router]
  );

  return (
    <div className="flex h-screen overflow-hidden">
      <MeetingSidebar />

      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <header className="flex items-start justify-between border-b border-[#E2E8F0] bg-white px-6 py-4">
          <div className="flex items-start gap-3">
            <button
              type="button"
              onClick={() => router.push("/meetings")}
              aria-label="回會議工作台"
              className="mt-0.5 inline-flex h-8 w-8 items-center justify-center rounded-lg border border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F8FAFC] hover:text-[#1A1A2E]"
            >
              <ArrowLeft size={16} />
            </button>
            <div>
              <h1 className="text-[20px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                {t("meetings.newMeetingTitle")}
              </h1>
              <p className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.newMeetingDesc")}</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={busy}
              onClick={() => void submit(true)}
              className="rounded-xl border border-[#E2E8F0] bg-white px-3.5 py-2 text-[13px] font-medium text-[#1A1A2E] hover:bg-[#F8FAFC] disabled:opacity-60"
            >
              {t("meetings.saveDraft")}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void submit(false)}
              className="rounded-xl bg-[#1A1A2E] px-3.5 py-2 text-[13px] font-medium text-white hover:bg-[#243149] disabled:opacity-60"
            >
              {t("meetings.sendInvitation")}
            </button>
          </div>
        </header>

        <div className="flex min-h-0 flex-1 gap-5 overflow-auto p-6">
          {/* Main form */}
          <section className="flex-1 space-y-5">
            <div className="rounded-2xl border border-[#E2E8F0] bg-white p-3">
              <label className="block text-[12px] font-medium text-[#475569]">
                連結專案（選填，連結後 action items 才能同步成任務）
              </label>
              <select
                value={projectId ?? ""}
                onChange={(e) => setProjectId(e.target.value || null)}
                className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
              >
                <option value="">— 不連結專案 —</option>
                {allProjects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
              {projectId && projectName && (
                <div className="mt-1 text-[11px] text-[#0050A0]">
                  ● 已連結 <strong>{projectName}</strong>
                </div>
              )}
            </div>


            <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
              <div className="mb-1 text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                {t("meetings.basicInfo")}
              </div>
              <div className="mb-4 text-[12px] text-[#94A3B8]">{t("meetings.required")}</div>

              <div className="grid grid-cols-3 gap-4">
                <div className="col-span-2">
                  <label className="block text-[12px] font-medium text-[#475569]">{t("meetings.field.name")}</label>
                  <input
                    type="text"
                    value={title}
                    onChange={(e) => setTitle(e.target.value)}
                    className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                  />
                </div>
                <div>
                  <label className="block text-[12px] font-medium text-[#475569]">{t("meetings.field.importance")}</label>
                  <div className="mt-1 flex gap-2">
                    {(["normal", "important"] as const).map((imp) => (
                      <button
                        key={imp}
                        type="button"
                        onClick={() => setImportance(imp)}
                        className={cn(
                          "flex-1 rounded-xl border px-3 py-2 text-[13px] transition",
                          importance === imp
                            ? imp === "important"
                              ? "border-[#0050A0] bg-[#EFF6FF] text-[#0050A0]"
                              : "border-[#1A1A2E] bg-[#F8FAFC] text-[#1A1A2E]"
                            : "border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F8FAFC]"
                        )}
                      >
                        {t(`meetings.importance.${imp}`)}
                      </button>
                    ))}
                  </div>
                </div>
              </div>

              <div className="mt-4 grid grid-cols-4 gap-4">
                <DateField label={t("meetings.field.startDate")} value={startDate} onChange={setStartDate} type="date" />
                <DateField label={t("meetings.field.startTime")} value={startTime} onChange={setStartTime} type="time" />
                <DateField label={t("meetings.field.endTime")} value={endTime} onChange={setEndTime} type="time" />
                <DateField label={t("meetings.field.endDate")} value={endDate} onChange={setEndDate} type="date" />
              </div>

              <div className="mt-4 grid grid-cols-3 gap-4">
                <label className="flex items-center gap-2 rounded-xl border border-[#E2E8F0] px-3 py-2 text-[13px]">
                  <input
                    type="checkbox"
                    checked={allDay}
                    onChange={(e) => setAllDay(e.target.checked)}
                    className="h-4 w-4"
                  />
                  {t("meetings.field.allDay")}
                </label>
                <select
                  value={recurrence}
                  onChange={(e) => setRecurrence(e.target.value as MeetingRecurrence)}
                  className="rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[13px] focus:border-[#0050A0] focus:outline-none"
                >
                  <option value="none">{t("meetings.field.recurrence")}：{t("meetings.recurrence.none")}</option>
                  <option value="daily">{t("meetings.recurrence.daily")}</option>
                  <option value="weekly">{t("meetings.recurrence.weekly")}</option>
                  <option value="monthly">{t("meetings.recurrence.monthly")}</option>
                </select>
                <select
                  value={timezone}
                  onChange={(e) => setTimezone(e.target.value)}
                  className="rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[13px] focus:border-[#0050A0] focus:outline-none"
                >
                  <option value="Asia/Taipei">時區：GMT+8 · 台北</option>
                  <option value="Asia/Tokyo">時區：GMT+9 · 東京</option>
                  <option value="UTC">時區：UTC</option>
                </select>
              </div>

              <div className="mt-4">
                <label className="block text-[12px] font-medium text-[#475569]">
                  {t("meetings.field.location")}
                </label>
                <div className="mt-1 flex gap-2">
                  {(["physical", "online"] as const).map((mode) => (
                    <button
                      key={mode}
                      type="button"
                      onClick={() => {
                        setLocationMode(mode);
                        if (mode === "physical") setLocation("");
                      }}
                      className={cn(
                        "rounded-xl border px-3 py-1.5 text-[13px] transition",
                        locationMode === mode
                          ? "border-[#0050A0] bg-[#EFF6FF] text-[#0050A0]"
                          : "border-[#E2E8F0] bg-white text-[#475569] hover:bg-[#F8FAFC]"
                      )}
                    >
                      {mode === "physical" ? "實體" : "線上"}
                    </button>
                  ))}
                </div>
                {locationMode === "physical" ? (
                  <div className="mt-2">
                    <select
                      value={location}
                      onChange={(e) => setLocation(e.target.value)}
                      onFocus={() => void loadRooms()}
                      className="w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                    >
                      <option value="">
                        {roomsLoading ? "查詢中…" : "選擇可用會議室"}
                      </option>
                      {rooms.map((r) => (
                        <option
                          key={r.name}
                          value={r.name}
                          disabled={!r.available}
                        >
                          {r.name}
                          {r.available ? "（空閒）" : "（佔用中）"}
                        </option>
                      ))}
                    </select>
                    {!roomsLoading && rooms.length === 0 && (
                      <div className="mt-1 text-[11px] text-[#94A3B8]">
                        該時段查無會議室資料。
                      </div>
                    )}
                  </div>
                ) : (
                  <div className="mt-2">
                    <select
                      value={onlineProvider}
                      onChange={(e) =>
                        setOnlineProvider(e.target.value as typeof onlineProvider)
                      }
                      className="w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                    >
                      <option value="webex">Webex</option>
                      <option value="teams">Microsoft Teams</option>
                      <option value="meet">Google Meet</option>
                    </select>
                    <div className="mt-1 text-[11px] text-[#94A3B8]">
                      會議連結待開發；目前僅記錄選擇之平台。
                    </div>
                  </div>
                )}
              </div>

              <div className="mt-4">
                <label className="block text-[12px] font-medium text-[#475569]">
                  {t("meetings.field.attendees")}
                </label>
                <div className="mt-1 grid grid-cols-3 gap-2">
                  <select
                    value={attendeeDept}
                    onChange={(e) => setAttendeeDept(e.target.value)}
                    onFocus={() => {
                      if (departments.length === 0) void loadDepartments();
                    }}
                    className="rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[13px] focus:border-[#0050A0] focus:outline-none"
                  >
                    <option value="">全部部門</option>
                    {departments.map((d) => (
                      <option key={d.code} value={d.code}>
                        {d.code} {d.name}
                      </option>
                    ))}
                  </select>
                  <div className="relative col-span-2">
                    <input
                      type="text"
                      value={attendees}
                      onChange={(e) => {
                        setAttendees(e.target.value);
                        setShowSuggestions(true);
                      }}
                      onFocus={() => setShowSuggestions(true)}
                      onBlur={() => {
                        // Delay so click on a suggestion lands before the
                        // popover unmounts. 150ms is the usual safe value.
                        window.setTimeout(() => setShowSuggestions(false), 150);
                      }}
                      placeholder="輸入姓名、員工編號或英文名…用 ; 分隔"
                      className="w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                    />
                    {showSuggestions && employeeMatches.length > 0 && (
                      <ul className="absolute left-0 right-0 top-full z-10 mt-1 max-h-64 overflow-auto rounded-xl border border-[#E2E8F0] bg-white shadow-lg">
                        {employeeMatches.map((emp) => (
                          <li
                            key={emp.employee_no}
                            onMouseDown={(e) => {
                              e.preventDefault();
                              pickEmployee(emp);
                            }}
                            className="cursor-pointer px-3 py-2 text-[13px] hover:bg-[#F8FAFC]"
                          >
                            <div className="font-medium text-[#1A1A2E]">
                              {emp.name}
                              <span className="ml-2 text-[11px] text-[#94A3B8]">
                                {emp.employee_no}
                              </span>
                            </div>
                            <div className="text-[11px] text-[#64748B]">
                              {[emp.dept_name, emp.title, emp.email]
                                .filter(Boolean)
                                .join(" · ")}
                            </div>
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                </div>
              </div>

              <div className="mt-4">
                <label className="block text-[12px] font-medium text-[#475569]">會議介紹</label>
                <textarea
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  rows={2}
                  placeholder="會議內容、議題或會議目標..."
                  className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                />
              </div>

              {locationMode === "online" && (
                <div className="mt-4">
                  <label className="block text-[12px] font-medium text-[#475569]">
                    線上會議連結（選填）
                  </label>
                  <input
                    type="url"
                    value={joinUrl}
                    onChange={(e) => setJoinUrl(e.target.value)}
                    placeholder="https://..."
                    className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                  />
                </div>
              )}

              <div className="mt-4">
                <label className="block text-[12px] font-medium text-[#475569]">{t("meetings.field.notificationNote")}</label>
                <textarea
                  value={notificationNote}
                  onChange={(e) => setNotificationNote(e.target.value)}
                  rows={3}
                  placeholder={t("meetings.notificationPlaceholder")}
                  className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
                />
              </div>
            </div>

            <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
              <div className="mb-1 text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                {t("meetings.attachments.title")}
              </div>
              <div className="mb-4 text-[12px] text-[#94A3B8]">{t("meetings.attachments.optional")}</div>

              <div className="grid grid-cols-2 gap-4">
                <label className="flex cursor-pointer flex-col items-center justify-center rounded-xl border-2 border-dashed border-[#CBD5E1] bg-[#F8FAFC] py-8 transition hover:border-[#0050A0]">
                  <Upload size={20} className="text-[#94A3B8]" />
                  <span className="mt-2 text-[13px] font-medium text-[#1A1A2E]">
                    {t("meetings.attachments.upload")}
                  </span>
                  <span className="mt-1 text-[11px] text-[#94A3B8]">
                    {t("meetings.attachments.uploadHint")}
                  </span>
                  <input
                    type="file"
                    multiple
                    className="hidden"
                    onChange={(e) => {
                      const list = Array.from(e.target.files ?? []);
                      setFiles((prev) => [
                        ...prev,
                        ...list.map((f) => ({ name: f.name, status: "pending" as const })),
                      ]);
                    }}
                  />
                </label>

                <div className="space-y-2">
                  {files.map((f, i) => (
                    <div key={i} className="rounded-lg border border-[#E2E8F0] bg-white px-3 py-2 text-[13px]">
                      <span className="font-medium text-[#1A1A2E]">{f.name}</span>
                      <span className="ml-2 text-[#94A3B8]">·</span>
                      <span className={cn("ml-2 text-[11px]", f.status === "attached" ? "text-[#10B981]" : "text-[#F59E0B]")}>
                        {f.status === "attached" ? t("meetings.attachments.attached") : t("meetings.attachments.pending")}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            </div>

            {error && (
              <div className="rounded-lg border border-[#FCA5A5] bg-[#FEE2E2] px-3 py-2 text-[13px] text-[#991B1B]">
                {error}
              </div>
            )}
          </section>

          {/* Right panel */}
          <aside className="flex w-[400px] flex-shrink-0 flex-col gap-5 overflow-y-auto">
            <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
              <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                {t("meetings.schedule.title")}
              </div>
              <div className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.schedule.hint")}</div>

              <div className="mt-4">
                <WeeklyMiniCalendar selectedDate={startDateObj} />
              </div>

              <div className="mt-5">
                <div className="mb-2 text-[13px] font-semibold text-[#1A1A2E]">
                  {t("meetings.schedule.today")}
                </div>
                <div className="space-y-1.5 text-[12px]">
                  <ScheduleRow time="09:30 - 10:30" label="產品週會" status="viewable" />
                  <ScheduleRow time="12:00 - 14:00" label="可安排" status="buildable" />
                  <ScheduleRow time="15:00 - 20:00" label="Busy" status="busy" />
                </div>
              </div>
            </div>

            <div className="rounded-2xl border border-[#E2E8F0] bg-white p-5">
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-[16px] font-semibold tracking-[-0.01em] text-[#1A1A2E]">
                    {t("meetings.recommended.title")}
                  </div>
                  <div className="mt-1 text-[12px] text-[#94A3B8]">{t("meetings.recommended.hint")}</div>
                </div>
                <span className="rounded-md border border-[#E2E8F0] px-2 py-0.5 text-[10px] text-[#64748B]">
                  {t("meetings.recommended.single")}
                </span>
              </div>

              <div className="mt-4">
                <TimeSlotPanel
                  slots={slots}
                  selectedStartIso={selectedSlotStart}
                  onPick={applySlot}
                />
              </div>
            </div>
          </aside>
        </div>
      </div>
    </div>
  );
}

function DateField({
  label,
  value,
  onChange,
  type = "text",
}: {
  label: string;
  value: string;
  onChange: (s: string) => void;
  type?: "text" | "date" | "time";
}) {
  return (
    <div>
      <label className="block text-[12px] font-medium text-[#475569]">{label}</label>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        step={type === "time" ? 300 : undefined}
        className="mt-1 w-full rounded-xl border border-[#E2E8F0] bg-white px-3 py-2 text-[14px] focus:border-[#0050A0] focus:outline-none"
      />
    </div>
  );
}

function ScheduleRow({
  time,
  label,
  status,
}: {
  time: string;
  label: string;
  status: "viewable" | "buildable" | "busy" | "noPermission";
}) {
  const t = useT();
  const color: Record<typeof status, string> = {
    viewable: "text-[#0050A0]",
    buildable: "text-[#10B981]",
    busy: "text-[#94A3B8]",
    noPermission: "text-[#94A3B8]",
  };
  return (
    <div className="flex items-center justify-between rounded-lg bg-[#F8FAFC] px-3 py-1.5">
      <span className="text-[#475569]">{time} · {label}</span>
      <span className={cn("font-medium", color[status])}>
        {t(`meetings.schedule.status.${status}`)}
      </span>
    </div>
  );
}
