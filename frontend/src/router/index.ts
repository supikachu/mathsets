import { createRouter, createWebHistory } from 'vue-router'
import Login from '@/views/Login.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'Login',
      component: Login,
      meta: { guest: true },
    },
    {
      path: '/register',
      name: 'Register',
      component: () => import('@/views/Register.vue'),
      meta: { guest: true },
    },
    {
      path: '/',
      component: () => import('@/components/AppLayout.vue'),
      meta: { requiresAuth: true },
      children: [
        {
          path: '',
          redirect: '/dashboard',
        },
        {
          path: 'dashboard',
          name: 'Dashboard',
          component: () => import('@/views/Dashboard.vue'),
        },
        {
          path: 'questions',
          name: 'Questions',
          component: () => import('@/views/QuestionList.vue'),
        },
        {
          path: 'questions/new',
          name: 'QuestionNew',
          component: () => import('@/views/QuestionEdit.vue'),
          meta: { immersive: true },
        },
        {
          path: 'questions/:id',
          name: 'QuestionDetail',
          component: () => import('@/views/QuestionDetail.vue'),
        },
        {
          path: 'questions/:id/edit',
          name: 'QuestionEdit',
          component: () => import('@/views/QuestionEdit.vue'),
          meta: { immersive: true },
        },
        {
          path: 'review',
          name: 'Review',
          component: () => import('@/views/ReviewQueue.vue'),
          // 审核队列权限由 Space 属性决定，不限制全局角色
        },
        {
          path: 'users',
          name: 'UserManagement',
          component: () => import('@/views/UserManagement.vue'),
          meta: { requiresAdmin: true },
        },
        {
          path: 'settings/tags',
          name: 'TagManagement',
          component: () => import('@/views/TagManagement.vue'),
          meta: { requiresAdmin: true },
        },
        {
          path: 'settings/knowledge-trees',
          name: 'KnowledgeTreeManagement',
          component: () => import('@/views/KnowledgeTreeManagement.vue'),
          meta: { requiresAdmin: true },
        },
        {
          // 修正 2：独立推库审批页面 —— 专供超级管理员处理 public_library_submissions
          // 与 ReviewQueue.vue 完全解耦：ReviewQueue 只负责空间内部审核
          path: 'admin/public-library-review',
          name: 'PublicLibraryReview',
          component: () => import('@/views/PublicLibraryReview.vue'),
          meta: { requiresAdmin: true },
        },
        {
          // V2.1.1 P1：标签候选审核
          path: 'admin/tag-candidates',
          name: 'TagCandidateReview',
          component: () => import('@/views/TagCandidateReview.vue'),
          meta: { requiresAdmin: true },
        },
        {
          path: 'collections/:id?',
          redirect: '/questions',
        },
        {
          path: 'profile',
          name: 'Profile',
          component: () => import('@/views/Profile.vue'),
        },
        {
          path: 'spaces/:id/settings',
          name: 'SpaceSettings',
          component: () => import('@/views/SpaceSettings.vue'),
        },
      ],
    },
    {
      path: '/:pathMatch(.*)*',
      name: 'NotFound',
      component: () => import('@/views/NotFound.vue'),
    },
  ],
})

// 路由守卫
router.beforeEach(async (to, _from) => {
  const { useAuthStore } = await import('@/stores/auth')
  const auth = useAuthStore()

  if (to.meta.requiresAuth && !auth.isLoggedIn) {
    return { path: '/login', query: { redirect: to.fullPath } }
  }

  if (to.meta.guest && auth.isLoggedIn) {
    return '/dashboard'
  }

  // 管理员路由：使用双轨统一判定（旧 isAdmin || 新 isSuperAdmin）
  if (to.meta.requiresAdmin && !auth.isAdminUnified) {
    return '/dashboard'
  }

  // 认证页面：确保空间列表已加载（兜底初始化）
  if (to.meta.requiresAuth && auth.isLoggedIn) {
    const { useSpaceStore } = await import('@/stores/space')
    const spaceStore = useSpaceStore()
    if (!spaceStore.spacesLoaded && !spaceStore.loading) {
      spaceStore.fetchSpaces()
    }
  }
})

export default router
