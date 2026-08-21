@echo off
setlocal
cd /d "%~dp0.."
echo ============================================================
echo  MathSet Data Cleanup (visualtest 7d questions + all papers)
echo ============================================================
powershell -ExecutionPolicy Bypass -File "%~dp0clean_visualtest_questions_and_all_papers.ps1" %*
pause
