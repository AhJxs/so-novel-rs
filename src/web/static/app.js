// 生成 inline SVG 图标
function svg(d) {
  return '<svg class="w-full h-full" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="' + d + '"/></svg>';
}

function app() {
  return {
    // 认证
    authenticated: false,
    authChecked: false,
    authCode: '',
    authError: '',

    // 导航
    page: 'search',
    mobileMenu: false,
    navItems: [
      { id: 'search', label: '搜索', icon: svg('M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z') },
      { id: 'tasks', label: '任务', icon: svg('M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4') },
      { id: 'library', label: '书库', icon: svg('M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8') },
      { id: 'sources', label: '书源', icon: svg('M4 6h16M4 10h16M4 14h16M4 18h16') },
      { id: 'settings', label: '设置', icon: svg('M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065zM15 12a3 3 0 11-6 0 3 3 0 016 0z') },
    ],

    // 搜索
    keyword: '',
    searching: false,
    searched: false,
    searchResults: [],
    searchSources: [],
    searchPage: 1,
    // 指定源搜索：'' = 聚合（默认），非空字符串 = 仅搜此源（来源 /api/sources 的 id 字段是 i32，前端以字符串承载）
    selectedSourceId: '',

    // 封面 URL 协议归一：把 http/https 替换成 //，避免 mixed content 被浏览器静默拦截
    get coverUrl() {
      const u = this.detailBook?.cover_url;
      if (!u) return '';
      return u.replace(/^https?:/, '');
    },

    // 响应式页大小：< sm 手机 12 / sm-md 平板 18 / md+ 桌面 24
    get pageSize() {
      const w = window.innerWidth;
      if (w >= 768) return 24;  // md+ 桌面
      if (w >= 640) return 18;  // sm 平板
      return 12;                // < sm 手机
    },

    // 分页派生属性
    get totalPages() {
      return Math.max(1, Math.ceil(this.searchResults.length / this.pageSize));
    },
    get pagedResults() {
      const start = (this.searchPage - 1) * this.pageSize;
      return this.searchResults.slice(start, start + this.pageSize);
    },
    // 可见页码（当前页前后各 2 个，超出范围用 '...'）
    get visiblePages() {
      return this._buildPages(this.totalPages, this.searchPage);
    },

    // 书库分页
    get libraryTotalPages() {
      return Math.max(1, Math.ceil(this.library.length / this.pageSize));
    },
    get pagedLibrary() {
      const start = (this.libraryPage - 1) * this.pageSize;
      return this.library.slice(start, start + this.pageSize);
    },
    get libraryVisiblePages() {
      return this._buildPages(this.libraryTotalPages, this.libraryPage);
    },

    // 通用分页构造
    _buildPages(total, cur) {
      if (total <= 7) return Array.from({ length: total }, (_, i) => i + 1);
      const pages = [1];
      const lo = Math.max(2, cur - 2);
      const hi = Math.min(total - 1, cur + 2);
      if (lo > 2) pages.push('...');
      for (let i = lo; i <= hi; i++) pages.push(i);
      if (hi < total - 1) pages.push('...');
      pages.push(total);
      return pages;
    },

    // 详情
    detailBook: null,
    detailUrl: '',
    detailSourceId: 0,
    chapters: [],
    loadingToc: false,

    // 下载
    dlFormat: 'epub',
    downloading: false,
    dlProgress: false,
    dlStatusText: '',
    dlProgressText: '',
    dlPercent: 0,

    // 任务
    tasks: [],
    activeTasks: 0,

    // 书库
    library: [],
    libraryPage: 1,

    // 书源
    sources: [],

    // 设置
    settings: { ext_name: 'Epub', download_path: '', proxy_enabled: false, proxy_host: '127.0.0.1', proxy_port: 7890, concurrency: 50, max_retries: 3, enable_retry: true },
    // 访问码（独立于 settings，仅存内存）
    accessCode: '',
    accessCodeSet: false,

    // Toast
    toasts: [],
    toastId: 0,

    init() {
      // 检查是否需要访问码验证
      this.checkAuthGate();
      // 切换到任务页时立即刷新
      this.$watch('page', (val) => { if (val === 'tasks') this.loadTasks(); });
      // 有活跃任务时每 2 秒轮询任务状态
      setInterval(() => { if (this.activeTasks > 0) this.loadTasks(); }, 2000);
      // 监听窗口尺寸变化，让 pageSize 响应式生效
      window.addEventListener('resize', () => { this.searchPage = 1; });
    },

    async checkAuthGate() {
      try {
        const resp = await fetch('/api/auth/status');
        const data = await resp.json();
        this.accessCodeSet = data.required;
        if (!data.required || data.authenticated) {
          this.authenticated = true;
          this.loadAppData();
        }
      } catch {}
      this.authChecked = true;
    },

    loadAppData() {
      this.loadSettings();
      this.loadSources();
      this.loadLibrary();
      this.loadTasks();
    },

    async verifyAuth() {
      if (this.authCode.length < 6) return;
      try {
        const resp = await fetch('/api/auth', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ code: this.authCode }),
        });
        const data = await resp.json();
        if (data.ok) {
          this.authenticated = true;
          this.authError = '';
          this.loadAppData();
        } else {
          this.authError = '访问码错误';
        }
      } catch { this.authError = '验证失败'; }
      this.authCode = '';
    },

    toast(message, type = 'info') {
      const id = ++this.toastId;
      this.toasts.push({ id, message, type, show: true });
      setTimeout(() => { const t = this.toasts.find(x => x.id === id); if (t) t.show = false; }, 3000);
      setTimeout(() => { this.toasts = this.toasts.filter(x => x.id !== id); }, 3500);
    },

    async doSearch() {
      if (!this.keyword.trim() || this.searching) return;
      this.searching = true;
      this.searched = true;
      this.searchResults = [];
      this.searchPage = 1;
      this.searchSources = [];

      let url = '/api/search?keyword=' + encodeURIComponent(this.keyword);
      // 后端 source_id 期待 i32；下拉空字符串代表聚合搜索（不附加 source_id）
      const sid = parseInt(this.selectedSourceId, 10);
      if (Number.isFinite(sid) && sid > 0) {
        url += '&source_id=' + sid;
      }

      const es = new EventSource(url);
      es.addEventListener('result', (e) => {
        const data = JSON.parse(e.data);
        this.searchSources.push(data.source_name);
        if (data.results) this.searchResults.push(...data.results);
      });
      es.addEventListener('done', () => { es.close(); this.searching = false; });
      es.onerror = () => { es.close(); this.searching = false; };
    },

    async showDetail(r) {
      this.detailBook = null;
      this.detailUrl = r.url;
      this.detailSourceId = r.source_id;
      this.chapters = [];
      this.dlProgress = false;
      this.downloading = false;
      this.page = 'detail';
      try {
        const resp = await fetch('/api/book/detail?url=' + encodeURIComponent(r.url) + '&source_id=' + r.source_id);
        if (resp.ok) {
          this.detailBook = await resp.json();
          // 详情成功后自动加载章节目录，避免用户手动点"加载目录"
          this.loadToc();
        }
        else this.toast('获取详情失败: ' + await resp.text(), 'error');
      } catch (e) { this.toast('网络错误', 'error'); }
    },

    async loadToc() {
      this.loadingToc = true;
      try {
        const resp = await fetch('/api/book/toc?url=' + encodeURIComponent(this.detailUrl) + '&source_id=' + this.detailSourceId);
        if (resp.ok) { const data = await resp.json(); this.chapters = data.chapters || []; }
        else this.toast('获取目录失败', 'error');
      } catch (e) { this.toast('网络错误', 'error'); }
      this.loadingToc = false;
    },

    async startDownload() {
      if (this.downloading) return;
      this.downloading = true;
      this.dlProgress = true;
      this.dlStatusText = '准备下载...';
      this.dlProgressText = '';
      this.dlPercent = 0;

      let total = this.chapters.length || 1;
      let maxIndex = 0;

      try {
        const resp = await fetch('/api/download', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            url: this.detailUrl,
            source_id: this.detailSourceId,
            format: this.dlFormat,
          }),
        });

        // 流式读取 SSE 事件
        const reader = resp.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split('\n');
          buffer = lines.pop(); // 保留不完整的行

          for (const line of lines) {
            if (!line.startsWith('data: ')) continue;
            try {
              const ev = JSON.parse(line.slice(6));
              this._handleDownloadEvent(ev, total, maxIndex);
              // 更新 total（后端可能在 book_resolved 时给出准确值）
              if (ev.total && ev.total > 0) total = ev.total;
              if (ev.type === 'chapter_done') {
                maxIndex = Math.max(maxIndex, ev.index || 0);
              }
            } catch {}
          }
        }
      } catch (e) {
        this.toast('下载错误: ' + e.message, 'error');
      }

      this.downloading = false;
    },

    /** 处理单个 SSE 下载进度事件 */
    _handleDownloadEvent(ev, total, maxIndex) {
      switch (ev.type) {
        case 'book_resolved':
          this.dlStatusText = '正在下载: ' + (ev.book_name || '');
          this.dlProgressText = '0 / ' + total;
          break;
        case 'chapter_done': {
          const pct = Math.min(100, Math.round((maxIndex / total) * 100));
          this.dlPercent = Math.max(this.dlPercent, pct);
          this.dlProgressText = maxIndex + ' / ' + total;
          this.dlStatusText = ev.title || '下载中...';
          break;
        }
        case 'finished':
          this.dlPercent = 100;
          this.dlStatusText = '下载完成！';
          this.dlProgressText = '';
          this.toast('下载完成: ' + (ev.filename || ''), 'success');
          this.loadLibrary();
          this.loadTasks();
          break;
        case 'failed':
          this.dlStatusText = '下载失败';
          this.toast('下载失败: ' + (ev.reason || ''), 'error');
          break;
      }
    },

    async loadTasks() {
      try {
        const resp = await fetch('/api/tasks');
        if (resp.ok) { this.tasks = await resp.json(); this.activeTasks = this.tasks.length; }
      } catch {}
    },

    async cancelTask(id) {
      try { await fetch('/api/tasks/' + id + '/cancel', { method: 'POST' }); this.toast('已取消'); this.loadTasks(); }
      catch { this.toast('取消失败', 'error'); }
    },

    async loadLibrary() {
      try {
        const resp = await fetch('/api/library');
        if (resp.ok) { this.library = await resp.json(); this.libraryPage = 1; }
      } catch {}
    },

    async deleteFile(filename) {
      if (!confirm('确定删除 ' + filename + '？')) return;
      try {
        const resp = await fetch('/api/library/' + encodeURIComponent(filename), { method: 'DELETE' });
        if (resp.ok) { this.toast('已删除'); this.loadLibrary(); }
        else this.toast('删除失败', 'error');
      } catch {}
    },

    async loadSources() {
      try {
        const resp = await fetch('/api/sources');
        if (resp.ok) {
          this.sources = (await resp.json()).map(s => ({ ...s, _testing: false, _testResult: null }));
        }
      } catch {}
    },

    async toggleSource(s) {
      try {
        const resp = await fetch('/api/sources/' + s.id + '/toggle', { method: 'POST' });
        if (resp.ok) {
          const updated = await resp.json();
          s.enabled = updated.enabled;
          this.toast(updated.enabled ? '已启用' : '已禁用');
        }
      } catch { this.toast('操作失败', 'error'); }
    },

    async testSource(s) {
      s._testing = true;
      s._testResult = null;
      try {
        const resp = await fetch('/api/sources/' + s.id + '/test', { method: 'POST' });
        const data = await resp.json();
        s._testResult = { ok: data.ok, latency_ms: data.latency_ms, error: data.error };
      } catch (e) {
        s._testResult = { ok: false, latency_ms: 0, error: '网络错误: ' + e.message };
      }
      s._testing = false;
    },

    async loadSettings() {
      try {
        const resp = await fetch('/api/settings');
        if (resp.ok) this.settings = await resp.json();
      } catch {}
    },

    async saveSettings() {
      try {
        const resp = await fetch('/api/settings', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(this.settings),
        });
        if (resp.ok) this.toast('设置已保存', 'success');
        else this.toast('保存失败', 'error');
      } catch {}
    },

    async saveAccessCode() {
      try {
        const resp = await fetch('/api/access-code', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ access_code: this.accessCode }),
        });
        if (resp.ok) {
          this.accessCodeSet = this.accessCode.length > 0;
          this.accessCode = '';
          this.toast('访问码已保存', 'success');
        } else {
          this.toast('保存失败', 'error');
        }
      } catch { this.toast('保存失败', 'error'); }
    },

    formatSize(bytes) {
      if (bytes < 1024) return bytes + ' B';
      if (bytes < 1048576) return (bytes / 1024).toFixed(1) + ' KB';
      return (bytes / 1048576).toFixed(1) + ' MB';
    },

    formatDate(ts) {
      if (!ts) return '';
      return new Date(ts * 1000).toLocaleDateString('zh-CN');
    },
  };
}

