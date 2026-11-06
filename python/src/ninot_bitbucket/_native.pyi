from types import TracebackType
from typing import Any, Self

from httpx import Request

class Participant:
    def __init__(self, account_id: str = ..., approved: bool = ..., role: str = ...) -> None: ...
    @property
    def account_id(self) -> str: ...
    @property
    def approved(self) -> bool: ...
    @property
    def role(self) -> str: ...

class PullRequest:
    @property
    def id(self) -> int: ...
    @property
    def title(self) -> str: ...
    @title.setter
    def title(self, title: str) -> None: ...
    @property
    def description(self) -> str: ...
    @description.setter
    def description(self, description: str) -> None: ...
    def set_title(self, title: str) -> None: ...
    def set_description(self, description: str) -> None: ...
    def add_status(
        self, state: str, *, key: str = ..., name: str = ..., url: str = ...
    ) -> None: ...

class Repository:
    @property
    def full_name(self) -> str: ...
    @property
    def prs(self) -> list[PullRequest]: ...
    def add_file(self, path: str, content: str | bytes) -> None: ...
    def create_pull_request(
        self,
        pr_id: int,
        *,
        title: str = ...,
        description: str = ...,
        state: str = ...,
        draft: bool = ...,
        source_branch: str = ...,
        target_branch: str = ...,
        source_repo: str | None = ...,
        target_repo: str | None = ...,
        author_display_name: str = ...,
        author_nickname: str = ...,
        author_account_id: str = ...,
        participants: list[Participant] | None = ...,
        updated_on: str = ...,
        url: str | None = ...,
    ) -> PullRequest: ...

class State:
    def __init__(self) -> None: ...
    def add_repo(
        self,
        workspace: str,
        slug: str,
        *,
        full_name: str | None = ...,
        description: str = ...,
        default_branch: str = ...,
        is_private: bool = ...,
        language: str = ...,
        size: int = ...,
        updated_on: str = ...,
        clone_https: str | None = ...,
    ) -> Repository: ...
    def set_user(
        self,
        *,
        username: str = ...,
        display_name: str = ...,
        account_id: str = ...,
        uuid: str = ...,
    ) -> None: ...
    @property
    def credentials(self) -> tuple[str, str] | None: ...
    @credentials.setter
    def credentials(self, value: tuple[str, str] | None) -> None: ...
    @property
    def requests(self) -> list[Request]: ...
    @property
    def _repos(self) -> dict[str, Repository]: ...
    @property
    def applied_faults(self) -> list[dict[str, Any]]: ...
    def add_fault(
        self,
        *,
        method: str | None = ...,
        path: str | None = ...,
        nth: int | None = ...,
        kind: str = ...,
        payload: Any = ...,
        times: int = ...,
    ) -> None: ...
    def when(
        self, *, method: str | None = ..., path: str | None = ..., nth: int | None = ...
    ) -> FaultBuilder: ...
    def fail_requests(
        self,
        *,
        status: int = ...,
        path_contains: str | None = ...,
        times: int = ...,
        json: Any = ...,
        text: str | None = ...,
        headers: dict[str, str] | None = ...,
    ) -> None: ...
    def clear_faults(self) -> None: ...
    def snapshot(self) -> Any: ...
    def restore(self, snapshot: Any) -> None: ...

class FaultBuilder:
    def fail(
        self,
        status: int = ...,
        *,
        json: Any = ...,
        text: str | None = ...,
        headers: dict[str, str] | None = ...,
        retry_after: int | None = ...,
        once: bool = ...,
        times: int | None = ...,
    ) -> None: ...
    def corrupt(self, mode: str = ..., *, once: bool = ..., times: int | None = ...) -> None: ...
    def drop(self, *, once: bool = ..., times: int | None = ...) -> None: ...
    def timeout(self, *, delay: float = ..., once: bool = ..., times: int | None = ...) -> None: ...

class Server:
    def __init__(self, state: State | None = ...) -> None: ...
    @property
    def url(self) -> str: ...
    @property
    def state(self) -> State: ...
    @property
    def is_recording(self) -> bool: ...
    def start_profile(self) -> None: ...
    def stop_profile(self) -> dict[str, Any]: ...
    def __enter__(self) -> Self: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> bool: ...
    def stop(self) -> None: ...
